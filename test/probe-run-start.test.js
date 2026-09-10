import { afterEach, describe, expect, it, vi } from 'vitest'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { main, probeRunStart } from './e2e/support/probe-run-start.mjs'

const temporary = []
const originalExitCode = process.exitCode
const runId = '018f0000-0000-7000-8000-000000000201'
const otherRunId = '018f0000-0000-7000-8000-000000000202'
afterEach(() => {
  vi.restoreAllMocks()
  process.exitCode = originalExitCode
  for (const directory of temporary.splice(0)) fs.rmSync(directory, { recursive: true, force: true })
})

function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-run-probe-'))
  temporary.push(directory)
  const config = path.join(directory, 'config')
  const desktopLog = path.join(directory, 'driver-app.log')
  const marker = path.join(config, 'local-mode')
  const journalPath = path.join(directory, 'runs.sqlite3')
  const journal = new DatabaseSync(journalPath)
  journal.exec('PRAGMA journal_mode = WAL')
  journal.exec('CREATE TABLE events (run_id TEXT, event_type TEXT)')
  journal.close()
  fs.writeFileSync(desktopLog, 'desktop runtime client connected=true\n')
  const args = [path.join(directory, 'desktop'), path.join(directory, 'attach.sock'), directory, config, desktopLog]
  const append = (id = runId, type = 'run.started') => {
    const writer = new DatabaseSync(journalPath)
    writer.prepare('INSERT INTO events VALUES (?, ?)').run(id, type)
    writer.close()
  }
  const execute = (executable, commandArgs, options) => {
    expect(executable).toBe(args[0])
    expect(commandArgs).toEqual(['--probe-local-run', args[1]])
    expect(options.timeout).toBe(80_000)
    expect(options.killSignal).toBe('SIGKILL')
    expect(fs.readFileSync(marker, 'utf8')).toBe('1')
    append()
    return { status: 0, signal: null, stdout: `${runId}\n` }
  }
  return { args, config, marker, desktopLog, journalPath, append, execute }
}

const success = { status: 0, signal: null, stdout: `${runId}\n` }

function expectFailure(value, execute, message) {
  const stderr = vi.spyOn(process.stderr, 'write').mockImplementation(() => true)
  const stdout = vi.spyOn(process.stdout, 'write').mockImplementation(() => true)
  main(value.args, execute)
  expect(process.exitCode).toBe(1)
  expect(stderr).toHaveBeenCalledExactlyOnceWith(`run_start_probe=failed\nrun_start_probe_error=${message}\n`)
  expect(stdout).not.toHaveBeenCalled()
  expect(fs.existsSync(value.marker)).toBe(false)
}

describe('installed run-start probe', () => {
  it('requires a new row for the submitted run and restores local mode', () => {
    const value = fixture()
    probeRunStart(...value.args, value.execute)
    expect(fs.existsSync(value.marker)).toBe(false)
  })

  it('prints the success result without a failure diagnostic', () => {
    const value = fixture()
    const stdout = vi.spyOn(process.stdout, 'write').mockImplementation(() => true)
    const stderr = vi.spyOn(process.stderr, 'write').mockImplementation(() => true)
    main(value.args, value.execute)
    expect(stdout).toHaveBeenCalledExactlyOnceWith('run_start_probe=passed\n')
    expect(stderr).not.toHaveBeenCalled()
    expect(process.exitCode).toBe(originalExitCode)
    expect(fs.existsSync(value.marker)).toBe(false)
  })

  it('removes a marker after a partial write failure', () => {
    const value = fixture()
    vi.spyOn(fs, 'writeFileSync').mockImplementation(() => { throw new Error('The marker write failed.') })
    expect(() => probeRunStart(...value.args, value.execute)).toThrow('The marker write failed.')
    expect(fs.existsSync(value.marker)).toBe(false)
  })

  it('preserves an existing local-mode marker', () => {
    const value = fixture()
    fs.mkdirSync(value.config)
    fs.writeFileSync(value.marker, '1')
    probeRunStart(...value.args, value.execute)
    expect(fs.readFileSync(value.marker, 'utf8')).toBe('1')
  })

  it.each([
    ['no journal row', () => {}, 0],
    ['an old row', () => {}, 0, true],
    ['another run', (value) => value.append(otherRunId), 0],
    ['another event', (value) => value.append(runId, 'run.failed'), 0],
    ['duplicate rows', (value) => { value.append(); value.append() }, 2],
  ])('rejects an acknowledgment with %s', (_, update, count, oldRow) => {
    const value = fixture()
    if (oldRow) value.append()
    expectFailure(value, () => { update(value); return success },
      `The journal check failed: run_started_rows=${count}`)
  })

  it.each([
    [{ ...success, status: 1, stderr: 'The runtime refused the run.\n' },
      'status=1 signal=null error=null stderr_tail="The runtime refused the run.\\n"'],
    [{ ...success, status: null, signal: 'SIGKILL', error: new Error('timeout'), stderr: 'Waiting for the runtime.\n' },
      'status=null signal="SIGKILL" error="timeout" stderr_tail="Waiting for the runtime.\\n"'],
    [{ ...success, status: null, error: new Error('spawn ENOENT'), stderr: null },
      'status=null signal=null error="spawn ENOENT" stderr_tail=""'],
    [{ ...success, signal: 'SIGTERM' },
      'status=0 signal="SIGTERM" error=null stderr_tail=""'],
    [{ ...success, error: new Error('spawn failed') },
      'status=0 signal=null error="spawn failed" stderr_tail=""'],
  ])('prints the executable failure measurements: %j', (result, measurements) => {
    expectFailure(fixture(), () => result, `The executable check failed: ${measurements}`)
  })

  it('prints only the last 4096 stderr characters and escapes diagnostic lines', () => {
    const suffix = '\nrun_start_probe=passed\n"tail"'
    const tail = `${'x'.repeat(4096 - suffix.length)}${suffix}`
    expect(tail).toHaveLength(4096)
    expectFailure(fixture(), () => ({ ...success, status: 1, stderr: `discard this prefix${tail}` }),
      `The executable check failed: status=1 signal=null error=null stderr_tail=${JSON.stringify(tail)}`)
  })

  it.each(['', `${runId}\n${otherRunId}`, "' OR 1=1 --", 'not-a-run-id'])('prints the invalid run ID: %j', (stdout) => {
    expectFailure(fixture(), () => ({ ...success, stdout }),
      `The run ID check failed: run_id=${JSON.stringify(stdout)}`)
  })

  it.each([
    '',
    'desktop runtime client connected=false\n',
    'desktop runtime client connected=true\ndesktop runtime client connected=true\n',
    'desktop runtime client connected=true\ndesktop runtime client connected=false\n',
    'desktop runtime client connected=true\ndesktop runtime client connected=false\ndesktop runtime client connected=true\n',
  ])('rejects absent or unstable desktop connections', (log) => {
    const value = fixture()
    fs.writeFileSync(value.desktopLog, log)
    expectFailure(value, value.execute,
      `The connection check failed: connections=${JSON.stringify(log.split('\n').filter(Boolean))}`)
  })

  it('rejects a missing journal without creating one', () => {
    const value = fixture()
    fs.unlinkSync(value.journalPath)
    expect(() => probeRunStart(...value.args, value.execute)).toThrow()
    expect(fs.existsSync(value.journalPath)).toBe(false)
    expect(fs.existsSync(value.marker)).toBe(false)
  })

  it('rejects a marker symlink without changing its target', () => {
    const value = fixture()
    fs.mkdirSync(value.config)
    fs.symlinkSync(value.desktopLog, value.marker)
    expect(() => probeRunStart(...value.args, value.execute)).toThrow()
    expect(fs.readFileSync(value.desktopLog, 'utf8')).toBe('desktop runtime client connected=true\n')
  })

  it('rejects absent arguments and emits the failure result line', () => {
    const result = spawnSync(process.execPath, ['test/e2e/support/probe-run-start.mjs'], { encoding: 'utf8' })
    expect(result.status).toBe(1)
    expect(result.stderr).toContain('run_start_probe=failed\nrun_start_probe_error=The run-start probe requires five absolute paths.\n')
  })

  it.each(['passed', 'failed'])('runs after pairing and includes the %s result in the redacted envelope', (outcome) => {
    const runner = fs.readFileSync('test/e2e/runner/macos.sh', 'utf8')
    expect(runner.indexOf('node test/e2e/support/probe-run-start.mjs')).toBeGreaterThan(
      runner.indexOf('node test/e2e/support/probe-companion-pairing.mjs'),
    )
    expect(runner).toContain('"$runtime_endpoint" "$runtime_state" "$runtime_config" "$raw/driver-app.log" >"$raw/run-start.log" 2>&1 || status=1')
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-run-probe-envelope-'))
    temporary.push(directory)
    const raw = path.join(directory, 'raw')
    const safe = path.join(directory, 'safe')
    fs.mkdirSync(raw)
    const token = `ghp_${'a'.repeat(36)}`
    const value = fixture()
    const stderr = vi.spyOn(process.stderr, 'write').mockImplementation(() => true)
    const stdout = vi.spyOn(process.stdout, 'write').mockImplementation(() => true)
    main(value.args, outcome === 'passed' ? value.execute :
      () => ({ ...success, status: 1, stderr: `The runtime refused ${token}.\n` }))
    const stream = outcome === 'passed' ? stdout : stderr
    const log = stream.mock.calls.map(([text]) => text).join('')
    fs.writeFileSync(path.join(raw, 'run-start.log'), log)
    const result = spawnSync(process.execPath, ['test/e2e/support/redact.mjs', raw, safe], { encoding: 'utf8' })
    expect(result.status).toBe(0)
    expect(fs.readFileSync(path.join(safe, 'run-start.log'), 'utf8')).toBe(outcome === 'passed' ?
      'run_start_probe=passed\n' :
      'run_start_probe=failed\nrun_start_probe_error=The executable check failed: status=1 signal=null error=null stderr_tail="The runtime refused [REDACTED:github-token].\\n"\n',
    )
  })
})
