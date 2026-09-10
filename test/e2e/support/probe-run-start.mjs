import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import path from 'node:path'
import { DatabaseSync } from 'node:sqlite'
import { pathToFileURL } from 'node:url'

export function probeRunStart(executable, endpoint, profile, config, desktopLog, execute = spawnSync) {
  if (![executable, endpoint, profile, config, desktopLog].every((value) => typeof value === 'string' && path.isAbsolute(value))) {
    throw new Error('The run-start probe requires five absolute paths.')
  }
  const marker = path.join(config, 'local-mode')
  let createdMarker = false
  let journal
  try {
    journal = new DatabaseSync(path.join(profile, 'runs.sqlite3'), { readOnly: true })
    journal.exec('PRAGMA busy_timeout = 5000')
    const { lastRow } = journal.prepare('SELECT COALESCE(MAX(rowid), 0) AS lastRow FROM events').get()
    fs.mkdirSync(config, { recursive: true })
    try {
      const descriptor = fs.openSync(marker, 'wx', 0o600)
      createdMarker = true
      try {
        fs.writeFileSync(descriptor, '1')
      } finally {
        fs.closeSync(descriptor)
      }
    } catch (error) {
      if (error.code !== 'EEXIST' || !fs.lstatSync(marker).isFile()) throw error
    }

    // Node cannot claim the desktop peer identity. The installed executable owns this connection.
    const result = execute(executable, ['--probe-local-run', endpoint], {
      encoding: 'utf8', timeout: 80_000, killSignal: 'SIGKILL', maxBuffer: 64 * 1024,
    })
    if (result.error || result.status !== 0 || result.signal) {
      throw new Error(`The executable check failed: status=${result.status ?? null} signal=${JSON.stringify(result.signal ?? null)} error=${JSON.stringify(result.error?.message ?? null)} stderr_tail=${JSON.stringify((result.stderr ?? '').slice(-4096))}`)
    }
    const runId = result.stdout.trim()
    if (!/^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(runId)) {
      throw new Error(`The run ID check failed: run_id=${JSON.stringify(runId)}`)
    }
    const { count } = journal.prepare(
      "SELECT COUNT(*) AS count FROM events WHERE rowid > ? AND run_id = ? AND event_type = 'run.started'",
    ).get(lastRow, runId)
    if (count !== 1) throw new Error(`The journal check failed: run_started_rows=${count}`)

    const connections = fs.readFileSync(desktopLog, 'utf8').split(/\r?\n/)
      .filter((line) => line.startsWith('desktop runtime client connected='))
    if (connections.length !== 1 || connections[0] !== 'desktop runtime client connected=true') {
      throw new Error(`The connection check failed: connections=${JSON.stringify(connections)}`)
    }
  } finally {
    try {
      journal?.close()
    } finally {
      if (createdMarker) fs.unlinkSync(marker)
    }
  }
}

export function main(args, execute = spawnSync) {
  try {
    probeRunStart(...args, execute)
    process.stdout.write('run_start_probe=passed\n')
  } catch (error) {
    process.stderr.write(`run_start_probe=failed\nrun_start_probe_error=${error.message}\n`)
    process.exitCode = 1
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv.slice(2))
}
