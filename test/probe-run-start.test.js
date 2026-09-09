import { afterEach, describe, expect, it, vi } from 'vitest'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { probeRunStart } from './e2e/support/probe-run-start.mjs'

const temporary = []
const runId = '018f0000-0000-7000-8000-000000000201'
const otherRunId = '018f0000-0000-7000-8000-000000000202'
afterEach(() => {
  vi.restoreAllMocks()
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

describe('installed run-start probe', () => {
  it('requires a new row for the submitted run and restores local mode', () => {
    const value = fixture()
    probeRunStart(...value.args, value.execute)
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
    ['no journal row', () => {}],
    ['an old row', () => {}, true],
    ['another run', (value) => value.append(otherRunId)],
    ['another event', (value) => value.append(runId, 'run.failed')],
    ['duplicate rows', (value) => { value.append(); value.append() }],
  ])('rejects an acknowledgment with %s', (_, update, oldRow) => {
    const value = fixture()
    if (oldRow) value.append()
    expect(() => probeRunStart(...value.args, () => { update(value); return success }))
      .toThrow('The journal has no new run.started row for the probe.')
    expect(fs.existsSync(value.marker)).toBe(false)
  })

  it.each([
    { ...success, status: 1 },
    { ...success, status: null, signal: 'SIGKILL', error: new Error('timeout') },
    { ...success, stdout: '' },
    { ...success, stdout: `${runId}\n${otherRunId}` },
    { ...success, stdout: "' OR 1=1 --" },
  ])('rejects failed or malformed native output', (result) => {
    const value = fixture()
    expect(() => probeRunStart(...value.args, () => result)).toThrow()
    expect(fs.existsSync(value.marker)).toBe(false)
  })

  it.each([
    '',
    'desktop runtime client connected=true\ndesktop runtime client connected=false\n',
    'desktop runtime client connected=true\ndesktop runtime client connected=false\ndesktop runtime client connected=true\n',
  ])('rejects absent or unstable desktop connections', (log) => {
    const value = fixture()
    fs.writeFileSync(value.desktopLog, log)
    expect(() => probeRunStart(...value.args, value.execute)).toThrow('The desktop connection did not stay open')
    expect(fs.existsSync(value.marker)).toBe(false)
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
    expect(result.stderr).toContain('run_start_probe=failed\n')
  })

  it('runs after pairing and includes its result in the redacted envelope', () => {
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
    fs.writeFileSync(path.join(raw, 'run-start.log'), 'run_start_probe=passed\n')
    const result = spawnSync(process.execPath, ['test/e2e/support/redact.mjs', raw, safe], { encoding: 'utf8' })
    expect(result.status).toBe(0)
    expect(fs.readFileSync(path.join(safe, 'run-start.log'), 'utf8')).toBe('run_start_probe=passed\n')
  })
})
