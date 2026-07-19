import { afterEach, describe, expect, it } from 'vitest'
import { execFileSync, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const root = process.cwd()
const temporary = []
const temp = () => { const value = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-e2e-test-')); temporary.push(value); return value }
afterEach(() => { for (const value of temporary.splice(0)) fs.rmSync(value, { recursive: true, force: true }) })
const runNode = (script, args, options = {}) => spawnSync(process.execPath, [path.join(root, script), ...args], { encoding: 'utf8', ...options })

describe('nightly asset identity', () => {
  const sha = 'a'.repeat(40)
  const asset = { name: `nightly-${sha}-linux-muniment.deb`, id: 42 }
  const validate = (release, candidate = sha) => runNode('test/e2e/support/asset-identity.mjs', [candidate], { input: JSON.stringify(release) })
  it('accepts exactly one pinned asset', () => expect(validate({ target_commitish: sha, assets: [asset] }).stdout).toBe('42'))
  it.each([
    ['missing', { target_commitish: sha, assets: [] }],
    ['duplicate', { target_commitish: sha, assets: [asset, asset] }],
    ['mismatched release', { target_commitish: 'b'.repeat(40), assets: [asset] }],
  ])('rejects %s identity', (_name, release) => expect(validate(release).status).not.toBe(0))
  it('rejects a noncanonical SHA', () => expect(validate({ target_commitish: sha, assets: [asset] }, 'A'.repeat(40)).status).not.toBe(0))
})

describe('artifact redaction boundary', () => {
  const redact = (files, env = {}) => {
    const source = temp(); const destination = path.join(temp(), 'safe')
    for (const [name, data] of Object.entries(files)) fs.writeFileSync(path.join(source, name), data)
    return { result: runNode('test/e2e/support/redact.mjs', [source, destination], { env: { ...process.env, ...env } }), destination }
  }
  it('emits ordinary diagnostics', () => {
    const { result, destination } = redact({ 'wdio.log': 'ordinary failure\n' })
    expect(result.status).toBe(0); expect(fs.readFileSync(path.join(destination, 'wdio.log'), 'utf8')).toContain('ordinary')
  })
  it.each([
    ['injected text', { 'app.log': 'private-user' }, { MUNIMENT_E2E_USERNAME: 'private-user' }],
    ['header token', { 'driver.log': 'Authorization: Bearer abcdefghijklmnopqrstuvwxyz' }, {}],
    ['unapproved screenshot', { 'failure-current-window.png': Buffer.from('not safe') }, {}],
  ])('blocks %s before destination creation', (_name, files, env) => {
    const { result, destination } = redact(files, env)
    expect(result.status).not.toBe(0); expect(fs.existsSync(destination)).toBe(false)
  })
  it('blocks an injected value hidden in an approved screenshot file', () => {
    const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=', 'base64')
    const { result, destination } = redact({ '01-signed-out.png': Buffer.concat([png, Buffer.from('private-user')]) }, { MUNIMENT_E2E_USERNAME: 'private-user' })
    expect(result.status).not.toBe(0); expect(fs.existsSync(destination)).toBe(false)
  })
})

describe('desktop-ci payload extraction', () => {
  const markers = (body) => `=== DESKTOP-CI ARTIFACTS BEGIN ===\n${body}\n=== DESKTOP-CI ARTIFACTS END ===\n`
  const archive = (setup) => {
    const source = temp(); setup(source)
    return execFileSync('tar', ['-czf', '-', '-C', source, '.']).toString('base64')
  }
  const extract = (output) => {
    const file = path.join(temp(), 'output'); const destination = path.join(temp(), 'artifacts'); fs.writeFileSync(file, output)
    return spawnSync('bash', [path.join(root, 'test/e2e/support/extract-artifacts.sh'), file, destination], { encoding: 'utf8' })
  }
  it('accepts one strict payload', () => expect(extract(markers(archive((dir) => fs.writeFileSync(path.join(dir, 'app.log'), 'safe')))).status).toBe(0))
  it.each(['', 'junk', `${markers('junk')}${markers('junk')}`, `=== DESKTOP-CI ARTIFACTS END ===\n=== DESKTOP-CI ARTIFACTS BEGIN ===\njunk\n`])('rejects malformed or ambiguous markers/base64', (value) => expect(extract(value).status).not.toBe(0))
  it('rejects link members', () => expect(extract(markers(archive((dir) => fs.symlinkSync('/tmp', path.join(dir, 'link'))))).status).not.toBe(0))
  it('rejects traversal members', () => {
    const source = temp(); fs.writeFileSync(path.join(source, 'file'), 'unsafe')
    const encoded = execFileSync('tar', ['-czf', '-', '--transform=s,^,../,', '-C', source, 'file']).toString('base64')
    expect(extract(markers(encoded)).status).not.toBe(0)
  })
})

describe('cleanup failure accounting', () => {
  const phases = ['revoke-session', 'stop-wdio', 'stop-driver', 'stop-browser-driver', 'stop-app', 'remove-package', 'remove-state', 'redact-artifacts', 'remove-raw', 'remove-package-file', 'remove-auth-url', 'replace-artifacts', 'publish-artifacts', 'remove-safe']
  it.each(phases)('continues after and records a %s failure', (failed) => {
    const ledger = path.join(root, 'test/e2e/support/cleanup-ledger.sh')
    const script = `cleanup_log=$(mktemp); cleanup_status=0; source "$1"; for phase in ${phases.join(' ')}; do if [[ $phase == "$2" ]]; then cleanup_step "$phase" false; else cleanup_step "$phase" true; fi; done; grep -q "$2: failed" "$cleanup_log" && [[ $(wc -l <"$cleanup_log") == ${phases.length} && $cleanup_status == 1 ]]`
    expect(spawnSync('bash', ['-c', script, 'test', ledger, failed]).status).toBe(0)
  })
  it('routes every required finalizer phase through the ledger', () => {
    const runner = fs.readFileSync(path.join(root, 'test/e2e/runner/linux.sh'), 'utf8')
    for (const phase of phases) expect(runner).toContain(`cleanup_step ${phase}`)
    expect(runner).toContain("'[c]hromedriver.*9515'")
    expect(runner).toContain('package-gone package_absent')
  })
})
