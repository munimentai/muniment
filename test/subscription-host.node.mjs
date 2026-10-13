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
const expired = '[desktop-ci 23:21:56] FATAL: no slot after 7200s\n'
const building = '[desktop-ci 22:02:56] SSH up; starting repo build (timeout 2400s)\n'
const unavailable = 'The native desktop-ci runner for this platform is unavailable.'
const sudoDenied = 'The desktop-CI host denied permission to start the driver.'
const sourceSha = 'a'.repeat(40)
const leases = JSON.stringify([{ provider: 'openai-codex', access: 'private-access', account_id: 'private-account', expires_ms: Date.now() + 4 * 60 * 60_000 }])
const models = JSON.stringify(Array.from({ length: 4 }, (_, i) => ({ family: 'openai', id: `model-${i}` })))
const options = { sourceSha, platform: 'windows', subscriptionPlatform: 'windows', leases, models, repository: 'owner/repo', token: 'private-token', sshKey: 'private-key', knownHosts: 'host' }

function temporary(work) {
  const output = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-host-test-'))
  try { return work(output) } finally { fs.rmSync(output, { recursive: true, force: true }) }
}

for (const [name, ssh, reason] of [
  ['slot wait', { status: 124, stdout: waiting + '[subscription-host] timeout=slot-wait\n' }, 'The desktop-CI slot stayed busy for the 120-minute wait limit.'],
  ['guest timeout', { status: 124, stdout: waiting + acquired + building }, 'The desktop-CI guest exceeded its 40-minute build timeout.'],
  ['driver guest timeout', { status: 1, stdout: waiting + acquired + building + '[desktop-ci 22:42:56] BUILD FAILED (windows) rc=124\n' }, 'The desktop-CI guest exceeded its 40-minute build timeout.'],
  ['run timeout', { status: 124, stdout: waiting + acquired + '[subscription-host] timeout=run\n' }, 'The desktop-CI guest and artifact collection exceeded their 60-minute limit.'],
  ['startup timeout', { status: 124, stdout: '[subscription-host] timeout=startup\n' }, 'The desktop-CI driver did not report a slot result within the 180-minute limit.'],
  ['client timeout', { status: 255, error: Object.assign(new Error('spawnSync ssh ETIMEDOUT'), { code: 'ETIMEDOUT' }) }, 'The desktop-CI SSH session exceeded its 185-minute limit.'],
  ['sudo password refusal', { status: 1, stderr: 'sudo: a password is required\n' }, sudoDenied],
  ['merged sudo refusal', { status: 1, stdout: 'sudo: a password is required\n' }, sudoDenied],
  ['sudo policy refusal', { status: 1, stderr: "Sorry, user desktopci is not allowed to execute '/usr/bin/desktop-ci windows' as root on host.\n" }, sudoDenied],
  ['sudoers refusal', { status: 1, stderr: 'desktopci is not in the sudoers file.\n' }, sudoDenied],
  ['driver permission refusal', { status: 1, stderr: 'sudo: unable to execute /usr/bin/desktop-ci: Permission denied\n' }, sudoDenied],
  ['shell permission refusal', { status: 126, stderr: 'sh: 1: python3: Permission denied\n' }, sudoDenied],
  ['guest sudo refusal', { status: 1, stdout: acquired + building, stderr: 'sudo: a password is required\n' }, unavailable],
  ['successful sudo diagnostic', { status: 0, stderr: 'sudo: a password is required\n' }, unavailable],
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
    'The desktop-CI slot stayed busy for the 120-minute wait limit.')
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
  assert.deepEqual(runDesktopCi({ ...options, output, spawnProcess(command, args, config) {
    if (command === 'ssh') {
      assert.equal(config.timeout, desktopCiBudget.client * 1000)
      assert.ok(args.includes('ConnectTimeout=30'))
      assert.ok(args.at(-1).startsWith('python3 -c '))
      assert.ok(args.at(-1).includes(` ${desktopCiBudget.slot} ${desktopCiBudget.run} ${desktopCiBudget.cleanup} sudo -n desktop-ci windows`))
      assert.ok(args.at(-1).includes(`--build-timeout ${desktopCiBudget.build} --collect-artifacts`))
      return { status: 0 }
    }
    return { status: 0 }
  } }), { status: 0 })
}))

test('the remote shell elevates only desktop-ci and preserves driver arguments and secret stdin', () => temporary(output => {
  const sudo = path.join(output, 'sudo')
  fs.writeFileSync(sudo, '#!/usr/bin/env python3\nimport os, sys\nif sys.argv[1:3] != ["-n", "desktop-ci"]:\n    sys.stderr.write("sudo: a password is required\\n")\n    sys.exit(1)\nos.execvp(sys.argv[2], sys.argv[2:])\n', { mode: 0o700 })
  const denied = spawnSync(sudo, ['-n', 'python3', '-c', 'print("unexpected")'], { encoding: 'utf8', timeout: 10_000 })
  assert.equal(denied.status, 1)
  assert.equal(denied.stdout, '')
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

test('the driver slot deadline stops the queued process and drains cleanup output', () => {
  const result = budgetProcess(`import signal, sys, time
signal.signal(signal.SIGTERM, lambda *_: (print("cleanup", flush=True), sys.exit(0)))
print(${JSON.stringify(waiting)}, end="", flush=True)
time.sleep(0.8)
print(${JSON.stringify(expired)}, end="", flush=True)
time.sleep(10)
print("guest started", flush=True)
`)
  assert.equal(result.error, undefined)
  assert.equal(result.status, 124)
  assert.match(result.stdout, /cleanup\n/)
  assert.match(result.stdout, /::error::The desktop-CI runner at 10\.1\.10\.10 is busy \(blocked\)\. The slot-wait budget expired\.\n\n\[subscription-host\] timeout=slot-wait\n$/)
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
time.sleep(0.8)
print(${JSON.stringify(expired)}, end="", flush=True)
time.sleep(10)
`)
  assert.equal(result.error, undefined)
  assert.equal(result.status, 124)
  assert.match(result.stdout, /\[subscription-host\] timeout=slot-wait\n$/)
})

test('the wrapper sends SIGTERM and escalates through the sudo process', () => temporary(output => {
  const escalation = path.join(output, 'escalation')
  fs.writeFileSync(path.join(output, 'sudo'), `#!/usr/bin/env python3
import signal, sys, time
assert sys.argv[1:3] == ["-n", "desktop-ci"]
signal.signal(signal.SIGTERM, lambda *_: print("sudo received SIGTERM", flush=True))
def escalate(*_):
    with open(${JSON.stringify(escalation)}, "w") as event:
        event.write("SIGALRM")
    sys.exit(0)
signal.signal(signal.SIGALRM, escalate)
print(${JSON.stringify(waiting)}, end="", flush=True)
time.sleep(0.8)
print(${JSON.stringify(expired)}, end="", flush=True)
time.sleep(10)
`, { mode: 0o700 })
  const result = spawnSync('python3', ['test/e2e/support/desktop-ci-budget.py', '0.8', '1.6', '0.3', 'sudo', '-n', 'desktop-ci'], {
    encoding: 'utf8', timeout: 10_000, env: { ...process.env, PATH: `${output}:${process.env.PATH}` },
  })
  assert.equal(result.error, undefined)
  assert.equal(result.status, 124, result.stderr + result.stdout)
  assert.match(result.stdout, /sudo received SIGTERM\n/)
  assert.match(result.stdout, /\[subscription-host\] timeout=slot-wait\n/)
  assert.equal(fs.readFileSync(escalation, 'utf8'), 'SIGALRM')
}))

function sudoFixture(remote, output, privilegedWrapper = false, input = 'private-stdin\n') {
  const config = { encoding: 'utf8', timeout: 10_000, input }
  if (process.getuid() !== 0) return spawnSync('sh', ['-c', remote], {
    ...config, env: { ...process.env, PATH: `${output}:${process.env.PATH}` },
  })

  const user = path.basename(output).replace('subscription-host-test-', 'subhost-').toLowerCase()
  const policy = `/etc/sudoers.d/${user}`
  const command = path.join(output, privilegedWrapper ? 'command' : 'desktop-ci')
  function checked(program, args) {
    const result = spawnSync(program, args, { encoding: 'utf8', timeout: 10_000 })
    assert.equal(result.error, undefined)
    assert.equal(result.status, 0, result.stderr + result.stdout)
    return result
  }

  // Only this root-owned command may cross the fixture's privilege boundary.
  fs.chmodSync(output, 0o755)
  if (privilegedWrapper) {
    assert.ok(remote.startsWith('sudo -n '))
    fs.writeFileSync(command, `#!/bin/sh\nPATH=/usr/bin:/bin\nexport PATH\nexec ${remote.slice('sudo -n '.length)}\n`, { mode: 0o755 })
  }
  checked('useradd', ['--system', '--no-create-home', '--no-user-group', '--no-log-init', '--shell', '/usr/sbin/nologin', user])
  let policyCreated = false
  try {
    fs.writeFileSync(policy, `Defaults:${user} secure_path="${output}:/usr/sbin:/usr/bin:/sbin:/bin"\n${user} ALL=(root) NOPASSWD: ${command}${privilegedWrapper ? ' ""' : ''}\n`, { mode: 0o440, flag: 'wx' })
    policyCreated = true
    checked('visudo', ['-cf', policy])
    const uid = checked('runuser', ['-u', user, '--', 'id', '-u'])
    assert.notEqual(Number(uid.stdout.trim()), 0)
    const denied = spawnSync('runuser', ['-u', user, '--', 'sudo', '-n', '/usr/bin/true'], config)
    assert.equal(denied.error, undefined)
    assert.notEqual(denied.status, 0, 'The fixture must not allow unrelated sudo commands.')
    const deniedPython = spawnSync('runuser', ['-u', user, '--', 'sudo', '-n', 'python3', '-c', 'print("unexpected")'], config)
    assert.equal(deniedPython.error, undefined)
    assert.notEqual(deniedPython.status, 0, 'The fixture must not elevate Python.')
    const invocation = privilegedWrapper ? ['sudo', '-n', command] : ['sh', '-c', remote]
    return spawnSync('runuser', ['-u', user, '--', ...invocation], {
      ...config, cwd: output, env: { PATH: `${output}:/usr/sbin:/usr/bin:/sbin:/bin` },
    })
  } finally {
    try {
      if (policyCreated) fs.rmSync(policy, { force: true })
    } finally {
      checked('userdel', [user])
    }
  }
}

for (const mode of ['success', 'cleanup', 'escalation']) {
  test(`the unprivileged wrapper runs with desktop-ci-only sudoers during ${mode}`, () => temporary(output => {
    fs.writeFileSync(path.join(output, 'desktop-ci'), `#!/usr/bin/python3
import os, signal, sys, time
assert os.geteuid() == 0
assert int(os.environ["SUDO_UID"]) != 0
assert sys.stdin.read().startswith("MUNIMENT_PI_CANDIDATE=1\\n")
assert sys.argv[1:3] == ["windows", "--console-user"]
print(f"guest reached driver={os.getpid()}", flush=True)
if ${JSON.stringify(mode)} == "success":
    sys.exit(0)
def cleanup(*_):
    print("cleanup", flush=True)
    sys.exit(0)
signal.signal(signal.SIGTERM, cleanup if ${JSON.stringify(mode)} == "cleanup" else signal.SIG_IGN)
signal.signal(signal.SIGHUP, signal.SIG_IGN)
print(${JSON.stringify(waiting)}, end="", flush=True)
time.sleep(0.8)
print(${JSON.stringify(expired)}, end="", flush=True)
time.sleep(10)
print("driver survived", flush=True)
`, { mode: 0o755 })
    assert.deepEqual(runDesktopCi({ ...options, output, spawnProcess(command, args, config) {
      if (command !== 'ssh') return { status: 0 }
      const remote = args.at(-1).replace(
        ` ${desktopCiBudget.slot} ${desktopCiBudget.run} ${desktopCiBudget.cleanup} `, ' 0.8 1.6 0.3 ')
      const result = sudoFixture(remote, output, false, config.input)
      assert.equal(result.error, undefined)
      assert.equal(result.status, mode === 'success' ? 0 : 124, result.stderr + result.stdout)
      assert.match(result.stdout, /guest reached driver=\d+\n/)
      assert.doesNotMatch(result.stdout + result.stderr, /driver survived|PermissionError/)
      const pid = result.stdout.match(/driver=(\d+)/)[1]
      const state = `/proc/${pid}/status`
      if (fs.existsSync(state)) assert.match(fs.readFileSync(state, 'utf8'), /State:\s+Z/)
      if (mode !== 'success') {
        assert.match(result.stdout, /\[subscription-host\] timeout=slot-wait\n/)
        if (mode === 'cleanup') assert.match(result.stdout, /cleanup\n/)
      }
      return result
    } }), mode === 'success' ? { status: 0 } : { status: 1, failure: 'slot-wait' })
  }))
}

for (const parentExits of [false, true]) {
  test(`direct privileged cleanup stops the root driver and its child when the driver ${parentExits ? 'exits' : 'ignores SIGTERM'}`, () => temporary(output => {
    const script = `import os, signal, sys, time
assert os.geteuid() == 0
assert int(os.environ["SUDO_UID"]) != 0
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
time.sleep(0.8)
print(${JSON.stringify(expired)}, end="", flush=True)
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
      .replace(' sudo -n desktop-ci windows', ` python3 -u '${driver}' windows`)
    const result = sudoFixture(`sudo -n ${remote}`, output, true)
    const privileged = (command, args, config) => process.getuid() === 0
      ? spawnSync(command, args, config)
      : spawnSync('sudo', ['-n', command, ...args], config)
    const pids = result.stdout?.match(/driver=(\d+) child=(\d+)/)?.slice(1) ?? []
    try {
      assert.equal(result.error, undefined)
      assert.equal(result.status, 124, result.stderr + result.stdout)
      assert.match(result.stdout, /\[subscription-host\] timeout=slot-wait\n$/)
      assert.doesNotMatch(result.stdout, /guest started/)
      assert.equal(pids.length, 2)
      const check = privileged('python3', ['-c', `import pathlib, sys, time
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
      if (pids.length) privileged('kill', ['-KILL', '--', ...pids], { timeout: 5000 })
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
