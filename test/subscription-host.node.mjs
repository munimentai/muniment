import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawn, spawnSync } from 'node:child_process'
import { desktopCiBudget, host, runDesktopCi } from './e2e/runner/subscription-host.mjs'
import { writeBlocked } from './e2e/runner/subscriptions.mjs'

const waiting = '[desktop-ci 21:21:56] waiting for a desktop-CI slot (lock)...\n'
const acquired = '[desktop-ci 22:01:56] slot 0 acquired\n'
const building = '[desktop-ci 22:02:56] SSH up; starting repo build (timeout 2400s)\n'
const unavailable = 'The native desktop-ci runner for this platform is unavailable.'
const sourceSha = 'a'.repeat(40)
const leases = JSON.stringify([{ provider: 'openai-codex', access: 'private-access', account_id: 'private-account', expires_ms: Date.now() + 4 * 60 * 60_000 }])
const models = JSON.stringify(Array.from({ length: 4 }, (_, i) => ({ family: 'openai', id: `model-${i}` })))
const options = { sourceSha, platform: 'windows', subscriptionPlatform: 'windows', leases, models, repository: 'owner/repo', token: 'private-token', sshKey: 'private-key', knownHosts: 'host' }

function temporary(work) {
  const output = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-host-test-'))
  try { return work(output) } finally { fs.rmSync(output, { recursive: true, force: true }) }
}

for (const [name, ssh, reason] of [
  ['slot wait', { status: 124, stdout: waiting + '[subscription-host] timeout=slot-wait\n' }, 'The desktop-CI slot stayed busy for the 60-minute wait limit.'],
  ['guest timeout', { status: 124, stdout: waiting + acquired + building }, 'The desktop-CI guest exceeded its 40-minute build timeout.'],
  ['driver guest timeout', { status: 1, stdout: waiting + acquired + building + '[desktop-ci 22:42:56] BUILD FAILED (windows) rc=124\n' }, 'The desktop-CI guest exceeded its 40-minute build timeout.'],
  ['run timeout', { status: 124, stdout: waiting + acquired + '[subscription-host] timeout=run\n' }, 'The desktop-CI guest and artifact collection exceeded their 60-minute limit.'],
  ['startup timeout', { status: 124, stdout: '[subscription-host] timeout=startup\n' }, 'The desktop-CI driver did not start within the 60-minute limit.'],
  ['client timeout', { status: 255, error: Object.assign(new Error('spawnSync ssh ETIMEDOUT'), { code: 'ETIMEDOUT' }) }, 'The desktop-CI SSH session exceeded its 125-minute limit.'],
  ['unreachable host', { status: 255, stderr: 'ssh: connect to host 10.1.10.10 port 22: Connection timed out\n' }, unavailable],
  ['missing ssh', { status: null, error: Object.assign(new Error('spawnSync ssh ENOENT'), { code: 'ENOENT' }) }, unavailable],
  ['disconnect during the slot wait', { status: 255, stdout: waiting, stderr: 'Connection closed\n' }, unavailable],
  ['unconfirmed timeout', { status: 124, stdout: '' }, unavailable],
  ['guest failure', { status: 1, stdout: waiting + acquired + building }, unavailable],
  ['contradictory timeout marker', { status: 0, stdout: '[subscription-host] timeout=slot-wait\n' }, unavailable],
]) {
  test(`the host records a fixed reason for the ${name}`, t => temporary(output => {
    t.mock.method(console, 'error', () => {})
    const status = host({ ...options, output, invoke: args => runDesktopCi({ ...args, spawnProcess(command) {
      return command === 'ssh' ? ssh : { status: 1, stderr: 'invalid desktop-ci artifact markers\n' }
    } }) })
    assert.equal(status, 1)
    const evidence = JSON.parse(fs.readFileSync(path.join(output, 'windows-subscription.json'), 'utf8'))
    assert.equal(evidence.status, 'blocked')
    assert.equal(evidence.reason, reason)
    const proof = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'), 'utf8'))
    assert.ok(proof.cases.length > 0)
    assert.ok(proof.cases.every(item => item.status === 'blocked' && item.reason === reason))
  }))
}

test('the host keeps guest failure evidence after a guest timeout', t => temporary(output => {
  t.mock.method(console, 'error', () => {})
  assert.equal(host({ ...options, output, invoke: args => runDesktopCi({ ...args, spawnProcess(command) {
    if (command === 'ssh') return { status: 124, stdout: waiting + acquired + building }
    writeBlocked(args.output, sourceSha, 'windows', 'The installed probe did not finish.')
    return { status: 0 }
  } }) }), 1)
  assert.equal(JSON.parse(fs.readFileSync(path.join(output, 'windows-subscription.json'))).reason, 'The installed probe did not finish.')
}))

test('a slot timeout replaces stale passing evidence', t => temporary(output => {
  t.mock.method(console, 'error', () => {})
  assert.equal(host({ ...options, output, invoke: args => {
    fs.writeFileSync(path.join(args.output, 'windows-subscription.json'), JSON.stringify({ status: 'passed' }))
    fs.writeFileSync(path.join(args.output, 'release-acceptance.json'), JSON.stringify({ cases: [{ status: 'passed' }] }))
    return { status: 1, failure: 'slot-wait' }
  } }), 1)
  assert.equal(JSON.parse(fs.readFileSync(path.join(output, 'windows-subscription.json'))).reason,
    'The desktop-CI slot stayed busy for the 60-minute wait limit.')
}))

test('the client and native jobs leave time for the slot, guest, artifacts, and cleanup', () => temporary(output => {
  assert.ok(desktopCiBudget.run >= desktopCiBudget.build + 20 * 60)
  assert.ok(desktopCiBudget.client > desktopCiBudget.slot + desktopCiBudget.run + desktopCiBudget.cleanup)
  const workflow = fs.readFileSync('.github/workflows/subscriptions.yml', 'utf8')
  for (const platform of ['linux', 'windows', 'macos-x64']) {
    const job = workflow.split(`\n  ${platform}:\n`)[1].split(/\n  [\w-]+:\n/)[0]
    const minutes = Number(job.match(/timeout-minutes: (\d+)/)[1])
    assert.ok(minutes * 60 > desktopCiBudget.client + 60)
  }
  const nightly = fs.readFileSync('.github/workflows/nightly.yml', 'utf8')
  assert.match(nightly.split('\n  targeted-release-acceptance:\n')[1].split(/\n  [\w-]+:\n/)[0], /uses: .\/.github\/workflows\/subscriptions.yml/)
  assert.deepEqual(runDesktopCi({ ...options, output, spawnProcess(command, args, config) {
    if (command === 'ssh') {
      assert.equal(config.timeout, desktopCiBudget.client * 1000)
      assert.ok(args.includes('ConnectTimeout=30'))
      assert.ok(args.at(-1).startsWith('sudo -n python3 -c '))
      assert.ok(args.at(-1).includes(` ${desktopCiBudget.slot} ${desktopCiBudget.run} ${desktopCiBudget.cleanup} desktop-ci windows`))
      assert.ok(args.at(-1).includes(`--build-timeout ${desktopCiBudget.build} --collect-artifacts`))
      return { status: 0 }
    }
    return { status: 0 }
  } }), { status: 0 })
}))

test('the remote shell preserves driver arguments and secret stdin', () => temporary(output => {
  const sudo = path.join(output, 'sudo')
  fs.writeFileSync(sudo, '#!/usr/bin/env python3\nimport os, sys\nassert sys.argv[1] == "-n"\nos.execvp(sys.argv[2], sys.argv[2:])\n', { mode: 0o700 })
  fs.writeFileSync(path.join(output, 'desktop-ci'), '#!/usr/bin/env python3\nimport json, sys\nprint(json.dumps({"args": ["desktop-ci", *sys.argv[1:]], "input": sys.stdin.read()}))\n', { mode: 0o700 })
  let transported
  assert.deepEqual(runDesktopCi({ ...options, output, platform: 'linux', spawnProcess(command, args, config) {
    if (command !== 'ssh') return { status: 0 }
    const result = spawnSync('sh', ['-c', args.at(-1)], {
      ...config, timeout: 10_000, env: { ...process.env, PATH: `${output}:${process.env.PATH}` },
    })
    assert.equal(result.status, 0, result.stderr)
    transported = JSON.parse(result.stdout)
    return result
  } }), { status: 0 })
  assert.deepEqual(transported.args.slice(0, 6), ['desktop-ci', 'linux', '--repo', 'https://github.com/owner/repo.git', '--ref', sourceSha])
  assert.equal(transported.args[6], '--cmd')
  assert.ok(transported.args[7].startsWith('sudo apt-get update -qq &&'))
  assert.ok(transported.args[7].endsWith('node test/e2e/runner/subscription-guest.mjs'))
  assert.deepEqual(transported.args.slice(8), ['--env-stdin', '--memory', '8192', '--build-timeout', '2400', '--collect-artifacts'])
  assert.ok(transported.input.includes(`MUNIMENT_SUBSCRIPTION_LEASES_BASE64=${Buffer.from(leases).toString('base64')}\n`))
  assert.ok(!transported.args.join(' ').includes('private-'))
}))

function budgetProcess(script, budgets = ['0.8', '1.6', '0.3']) {
  return spawnSync('python3', ['test/e2e/support/desktop-ci-budget.py', ...budgets, 'python3', '-u', '-c', script], {
    encoding: 'utf8', timeout: 10_000, input: 'private-stdin\n',
  })
}

test('the remote slot deadline stops the queued process and drains cleanup output', () => {
  const result = budgetProcess(`import signal, sys, time
signal.signal(signal.SIGTERM, lambda *_: (print("cleanup", flush=True), sys.exit(0)))
print(${JSON.stringify(waiting)}, end="", flush=True)
time.sleep(10)
print("guest started", flush=True)
`)
  assert.equal(result.error, undefined)
  assert.equal(result.status, 124)
  assert.match(result.stdout, /cleanup\n\n\[subscription-host\] timeout=slot-wait\n$/)
  assert.doesNotMatch(result.stdout, /guest started/)
})

test('the remote run gets a fresh budget after a fragmented slot marker', () => {
  const result = budgetProcess(`import sys, time
assert sys.stdin.read() == "private-stdin\\n"
print(${JSON.stringify(waiting)}, end="", flush=True)
time.sleep(0.5)
sys.stdout.write("[desktop-ci 22:01:56] slot ")
sys.stdout.flush()
time.sleep(0.05)
print("0 acquired", flush=True)
time.sleep(1.0)
print("artifacts collected", flush=True)
`)
  assert.equal(result.error, undefined)
  assert.equal(result.status, 0, result.stderr + result.stdout)
  assert.match(result.stdout, /artifacts collected\n$/)
  assert.doesNotMatch(result.stdout, /timeout=/)
})

test('repeated slot markers cannot extend the remote run deadline', () => {
  const result = budgetProcess(`import time
while True:
    print(${JSON.stringify(acquired)}, end="", flush=True)
    time.sleep(0.1)
`)
  assert.equal(result.error, undefined)
  assert.equal(result.status, 124)
  assert.match(result.stdout, /\[subscription-host\] timeout=run\n$/)
})

test('the remote deadline kills a driver that ignores SIGTERM', () => {
  const result = budgetProcess(`import signal, time
signal.signal(signal.SIGTERM, signal.SIG_IGN)
print(${JSON.stringify(waiting)}, end="", flush=True)
time.sleep(10)
`)
  assert.equal(result.error, undefined)
  assert.equal(result.status, 124)
  assert.match(result.stdout, /\[subscription-host\] timeout=slot-wait\n$/)
})

for (const parentExits of [false, true]) {
  test(`sudo cleanup stops the root driver and its child when the driver ${parentExits ? 'exits' : 'ignores SIGTERM'}`, () => temporary(output => {
    assert.notEqual(process.getuid(), 0, 'Run this test as an unprivileged user with passwordless sudo.')
    const script = `import os, signal, sys, time
assert os.geteuid() == 0
assert sys.stdin.read() == "private-stdin\\n"
signal.signal(signal.SIGTERM, signal.SIG_IGN)
child = os.fork()
if child == 0:
    os.close(1)
    os.close(2)
    time.sleep(10)
    os._exit(0)
print(f"driver={os.getpid()} child={child}", flush=True)
if ${parentExits ? 'True' : 'False'}:
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
print(${JSON.stringify(waiting)}, end="", flush=True)
time.sleep(10)
print("guest started", flush=True)
`
    const driver = path.join(output, 'driver.py')
    fs.writeFileSync(driver, script)
    let remote
    runDesktopCi({ ...options, output, spawnProcess(command, args) {
      if (command === 'ssh') remote = args.at(-1)
      return { status: 0 }
    } })
    remote = remote.replace(` ${desktopCiBudget.slot} ${desktopCiBudget.run} ${desktopCiBudget.cleanup} `, ' 0.8 1.6 0.3 ')
      .replace(' desktop-ci windows', ` python3 -u '${driver}' windows`)
    const result = spawnSync('sh', ['-c', remote], {
      encoding: 'utf8', timeout: 10_000, input: 'private-stdin\n',
    })
    const pids = result.stdout?.match(/driver=(\d+) child=(\d+)/)?.slice(1) ?? []
    try {
      assert.equal(result.error, undefined)
      assert.equal(result.status, 124, result.stderr + result.stdout)
      assert.match(result.stdout, /\[subscription-host\] timeout=slot-wait\n$/)
      assert.doesNotMatch(result.stdout, /guest started/)
      assert.equal(pids.length, 2)
      const check = spawnSync('sudo', ['-n', 'python3', '-c', `import pathlib, sys, time
for pid in sys.argv[1:]:
    status = pathlib.Path(f"/proc/{pid}/status")
    for _ in range(100):
        try:
            state = status.read_text()
        except FileNotFoundError:
            break
        if "State:\\tZ" in state:
            break
        time.sleep(0.01)
    else:
        sys.exit(f"Process {pid} survived the deadline.")
`, ...pids], { encoding: 'utf8', timeout: 5000 })
      assert.equal(check.error, undefined)
      assert.equal(check.status, 0, check.stderr)
    } finally {
      if (pids.length) spawnSync('sudo', ['-n', 'kill', '-KILL', '--', ...pids], { timeout: 5000 })
    }
  }))
}

test('an SSH hangup stops the queued driver', async () => {
  const script = `import signal, sys, time
signal.signal(signal.SIGTERM, lambda *_: (print("cleanup", flush=True), sys.exit(0)))
print(${JSON.stringify(waiting)}, end="", flush=True)
time.sleep(10)
`
  const child = spawn('python3', ['test/e2e/support/desktop-ci-budget.py', '10', '10', '0.3', 'python3', '-u', '-c', script], { timeout: 10_000 })
  let output = '', sent = false, stderr = ''
  child.stdout.on('data', chunk => {
    output += chunk
    if (!sent && output.includes(waiting)) {
      sent = true
      child.kill('SIGHUP')
    }
  })
  child.stderr.on('data', chunk => { stderr += chunk })
  const code = await new Promise((resolve, reject) => {
    child.on('error', reject)
    child.on('close', resolve)
  })
  assert.equal(code, 124, stderr)
  assert.match(output, /cleanup\n\n\[subscription-host\] timeout=interrupted\n$/)
})

test('the remote wrapper preserves failures and rejects invalid budgets', () => {
  const failed = budgetProcess('import sys\nsys.exit(124)')
  assert.equal(failed.status, 124)
  assert.equal(failed.stdout, '')
  for (const value of ['0', '-1', 'nan', 'inf', 'bad']) {
    const result = budgetProcess('print("guest started")', [value, '1', '1'])
    assert.notEqual(result.status, 0)
    assert.equal(result.stdout, '')
  }
})
