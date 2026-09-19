import { expect, it } from 'vitest'
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, readdirSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'

it.skipIf(process.platform === 'win32')('retains the complete envelope and stable reports, and rejects failed readback', () => {
  const root = mkdtempSync(join(tmpdir(), 'ci-artifacts-'))
  try {
    const source = join(root, 'source')
    const store = join(root, 'store')
    mkdirSync(source)
    mkdirSync(store)
    writeFileSync(join(source, 'junit-results.xml'), '<testsuites/>')
    writeFileSync(join(source, 'driver-app.log'), 'runtime failure evidence')
    writeFileSync(join(root, 'aws'), `#!/usr/bin/env node
const fs = require('node:fs'); const path = require('node:path');
const [source, dest] = process.argv.slice(6, 8);
const local = value => value.startsWith('s3://') ? path.join(process.env.STORE, value.slice(5)) : value;
fs.mkdirSync(path.dirname(local(dest)), { recursive: true });
fs.copyFileSync(local(source), local(dest));
if (source.startsWith('s3://') && process.env.CORRUPT) fs.appendFileSync(local(dest), 'corrupt');
`, { mode: 0o755 })
    const env = { ...process.env, PATH: `${root}:${process.env.PATH}`, STORE: store,
      AWS_ACCESS_KEY_ID: 'test', AWS_SECRET_ACCESS_KEY: 'test', GITHUB_RUN_ID: '42', GITHUB_RUN_ATTEMPT: '2',
      GITHUB_STEP_SUMMARY: join(root, 'summary') }
    const publish = () => spawnSync('bash', [resolve('.github/publish-ci-artifacts.sh'), 'windows', source], { env, encoding: 'utf8' })
    const result = publish()
    expect(result.status, result.stderr).toBe(0)
    const prefix = join(store, 'factory-ci-artifacts/muniment-desktop/42')
    expect(readdirSync(join(prefix, 'windows-e2e-report'))).toEqual(['junit-results.xml'])
    const archive = join(prefix, 'windows-e2e/attempt-2/diagnostics.tar.gz')
    const log = spawnSync('tar', ['-xOzf', archive, './driver-app.log'], { encoding: 'utf8' })
    expect(log.stdout).toBe('runtime failure evidence')
    expect(readFileSync(env.GITHUB_STEP_SUMMARY, 'utf8')).toContain('attempt-2/diagnostics.tar.gz')
    env.CORRUPT = '1'
    expect(publish().status).not.toBe(0)
    delete env.CORRUPT
    rmSync(join(source, 'junit-results.xml'))
    expect(publish().status).not.toBe(0)
  } finally { rmSync(root, { recursive: true, force: true }) }
})
