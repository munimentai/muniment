import { test } from 'node:test'
import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { desktopCiBudget } from './e2e/runner/subscription-host.mjs'

const helper = '.github/desktop-ci-command.py'
const reason = 'The desktop-CI runner at 10.1.10.10 is busy (blocked). The slot-wait budget expired.'
const waiting = '[desktop-ci 10:00:00] waiting for a desktop-CI slot (lock)...\n'
const acquired = '[desktop-ci 11:59:00] slot 0 acquired\n'
const invoke = (program, args, options = {}) => spawnSync(program, args, { encoding: 'utf8', timeout: 10_000, ...options })

function wrap(slot, remote) {
  const result = invoke('python3', [helper, String(slot), remote])
  assert.equal(result.status, 0, result.stderr)
  const decoded = invoke('python3', ['-c', 'import json, shlex, sys; print(json.dumps(shlex.split(sys.stdin.read())))'], { input: result.stdout })
  assert.equal(decoded.status, 0, decoded.stderr)
  return JSON.parse(decoded.stdout)
}

function checkJob(job) {
  const timeout = job.match(/^    timeout-minutes: (.+)$/m)?.[1]
  assert.ok(timeout, 'The desktop-CI job needs an explicit timeout.')
  if (job.includes('run: node test/e2e/runner/subscription-host.mjs')) {
    assert.ok(Number(timeout) * 60 >= desktopCiBudget.client + 600)
    assert.ok(desktopCiBudget.run >= desktopCiBudget.build + 1200)
    assert.ok(desktopCiBudget.client > desktopCiBudget.slot + desktopCiBudget.run + desktopCiBudget.cleanup)
    return
  }
  const slot = job.match(/DESKTOP_CI_SLOT_SECONDS: (\d+)/)?.[1]
  assert.ok(Number(slot) > 0, 'The slot-wait budget must be positive.')
  const commands = [...job.matchAll(/^ +(?:remote|ssh_cmd)="(sudo desktop-ci .+)"$/gm)]
  assert.ok(commands.length > 0)
  const wrappers = [...job.matchAll(/(remote|ssh_cmd)=\$\(python3 \.github\/desktop-ci-command.py "\$DESKTOP_CI_SLOT_SECONDS" "\$\1"\)/g)]
  assert.equal(wrappers.length, commands.length, 'Wrap every desktop-CI invocation.')
  assert.doesNotMatch(job, /desktopci@10\.1\.10\.10\s+"sudo/)
  assert.match(job, /uses: actions\/checkout@/)
  const attempts = job.includes('while true; do') ? Number(job.match(/"\$attempt" -ge (\d+)/)?.[1]) : 1
  assert.ok(Number.isInteger(attempts) && attempts > 0)
  for (const [, command] of commands) {
    const build = command.match(/--build-timeout (\d+|'\$build_timeout')/)?.[1]
    assert.ok(build, 'The guest needs an explicit build timeout.')
    const builds = build.startsWith("'") ? [...job.matchAll(/^ +build_timeout=(\d+)$/gm)].map(match => Number(match[1])) : [Number(build)]
    assert.ok(builds.length > 0)
    const installer = timeout.match(/^\$\{\{ needs.changes.outputs.installer == 'true' && (\d+) \|\| (\d+) \}\}$/)
    for (const seconds of builds) {
      // The installer branch sets the larger guest budget.
      const minutes = installer ? Number(installer[seconds === Math.max(...builds) ? 1 : 2]) : Number(timeout)
      const args = wrap(slot, `sudo desktop-ci linux --build-timeout ${seconds}`)
      const [slotSeconds, runSeconds, cleanupSeconds] = args.slice(3, 6).map(Number)
      assert.equal(slotSeconds, Number(slot))
      assert.ok(runSeconds >= seconds + 1200)
      assert.ok(cleanupSeconds > 0)
      assert.ok(minutes * 60 >= attempts * (slotSeconds + runSeconds + cleanupSeconds) + 600,
        `The ${minutes}-minute job cannot cover ${attempts} attempts with a ${seconds}-second guest.`)
    }
  }
}

const jobs = []
for (const file of fs.readdirSync('.github/workflows').filter(name => /\.ya?ml$/.test(name))) {
  const workflow = fs.readFileSync(`.github/workflows/${file}`, 'utf8')
  for (const match of workflow.matchAll(/^  ([\w-]+):\n([\s\S]*?)(?=^  [\w-]+:\n|(?![\s\S]))/gm)) {
    if (/sudo(?: -n)? desktop-ci|run: node test\/e2e\/runner\/subscription-host\.mjs|python3 \.github\/desktop-ci-command\.py/.test(match[2])) {
      jobs.push([`${file}/${match[1]}`, match[2]])
    }
  }
}

test('every desktop-CI job budgets the slot, guest, collection, cleanup, and upload', () => {
  assert.deepEqual(jobs.map(([name]) => name).sort(), [
    'ci.yml/attach-fixtures-current', 'ci.yml/desktop-compile', 'nightly.yml/build',
    'nightly.yml/linux-e2e', 'nightly.yml/macos-e2e', 'nightly.yml/windows-e2e',
    'subscriptions.yml/linux', 'subscriptions.yml/macos-x64', 'subscriptions.yml/windows',
  ])
  for (const [name, job] of jobs) {
    try { checkJob(job) } catch (error) { throw new Error(name, { cause: error }) }
  }
})

test('the workflow guard rejects short budgets and unwrapped or unbounded guests', () => {
  const job = jobs.find(([name]) => name === 'nightly.yml/build')[1]
  for (const invalid of [
    job.replace('timeout-minutes: 235', 'timeout-minutes: 90'),
    job.replace('DESKTOP_CI_SLOT_SECONDS: 7200', 'DESKTOP_CI_SLOT_SECONDS: 0'),
    job.replace('python3 .github/desktop-ci-command.py', 'echo'),
    job.replace("--build-timeout '$build_timeout'", ''),
  ]) assert.throws(() => checkJob(invalid))
  const preflight = jobs.find(([name]) => name === 'ci.yml/desktop-compile')[1]
  assert.throws(() => checkJob(preflight.replace('355 || 295', '355 || 45')))
  assert.throws(() => checkJob(preflight.replace('355 || 295', '120 || 295')))
  assert.throws(() => checkJob(preflight.replace('"$attempt" -ge 2', '"$attempt" -ge 3')))
})

test('the command wrapper preserves arguments and rejects invalid budgets before the driver starts', () => {
  const remote = `sudo desktop-ci macos --cmd 'printf "%s" "$secret" && echo done' --env-stdin --build-timeout 4800 --collect-artifacts`
  const args = wrap(7200, remote)
  assert.deepEqual(args.slice(3), ['7200', '6000', '30', 'sudo', '-n', 'desktop-ci', 'macos', '--cmd',
    'printf "%s" "$secret" && echo done', '--env-stdin', '--build-timeout', '4800', '--collect-artifacts'])
  for (const value of ['', '0', '-1', 'nan', 'inf', '1.5', '1; echo unsafe']) {
    assert.notEqual(invoke('python3', [helper, value, remote]).status, 0)
    assert.notEqual(invoke('python3', [helper, '7200', remote.replace('4800', value)]).status, 0)
  }
  for (const invalid of [remote.replace('sudo desktop-ci', 'sudo python3'), remote + ' --build-timeout 1', 'sudo desktop-ci linux --build-timeout']) {
    assert.notEqual(invoke('python3', [helper, '7200', invalid]).status, 0)
  }
})

test('a queued acquisition marker wins over an expired slot deadline', () => {
  const driver = `import os, signal, time
os.kill(os.getppid(), signal.SIGSTOP)
print(${JSON.stringify(waiting + acquired)}, end="", flush=True)
time.sleep(0.4)
os.kill(os.getppid(), signal.SIGCONT)
time.sleep(0.3)
print("guest finished", flush=True)
`
  const result = invoke('python3', ['test/e2e/support/desktop-ci-budget.py', '0.2', '1.6', '0.3', 'python3', '-u', '-c', driver])
  assert.equal(result.status, 0, result.stderr + result.stdout)
  assert.match(result.stdout, /guest finished/)
  assert.doesNotMatch(result.stdout, /blocked|timeout=/)
})

test('a slot timeout produces a failed report with a fixed blocked reason despite stale passing evidence', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'desktop-ci-budget-'))
  try {
    const result = invoke('python3', ['test/e2e/support/desktop-ci-budget.py', '0.2', '1', '0.3', 'python3', '-u', '-c',
      `import signal, sys, time\nsignal.signal(signal.SIGTERM, lambda *_: sys.exit(0))\nprint(${JSON.stringify(waiting)}, end="", flush=True)\ntime.sleep(10)`])
    assert.equal(result.status, 124, result.stderr + result.stdout)
    assert.ok(result.stdout.includes(reason))
    const transcript = path.join(root, 'output')
    const artifacts = path.join(root, 'artifacts')
    fs.mkdirSync(artifacts)
    fs.writeFileSync(path.join(artifacts, 'junit-stale.xml'), '<testsuites tests="1" failures="0"/>')
    fs.writeFileSync(transcript, result.stdout)
    const extract = invoke('bash', ['test/e2e/support/extract-artifacts.sh', transcript, artifacts, '124'])
    assert.equal(extract.status, 1)
    assert.equal(fs.readFileSync(path.join(artifacts, 'runner-failure.txt'), 'utf8').trim(), reason)
    const report = invoke('bash', ['test/e2e/support/ensure-junit-report.sh', artifacts, 'installed-macos', '124', '1'])
    assert.equal(report.status, 0, report.stderr)
    const xml = fs.readFileSync(path.join(artifacts, 'junit-infrastructure.xml'), 'utf8')
    assert.match(xml, /failures="1"/)
    assert.ok(xml.includes(reason))
  } finally {
    fs.rmSync(root, { recursive: true, force: true })
  }
})
