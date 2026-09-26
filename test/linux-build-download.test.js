import { describe, it, expect } from 'vitest'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { spawnSync } from 'node:child_process'

const script = readFileSync('.github/build-linux.sh', 'utf8')
const fetchTool = script.match(/fetch_tool\(\) \{[\s\S]*?\n\}/)[0]
const api = 'https://api.github.com/repos/tauri-apps/binary-releases/releases/assets/182515537'
const payload = 'pinned tool bytes'
const digest = createHash('sha256').update(payload).digest('hex')
const token = 'test-token-not-a-credential'

function fixture() {
  const directory = mkdtempSync(join(tmpdir(), 'linux-build-download-'))
  const destination = join(directory, 'tool')
  const log = join(directory, 'request.json')
  writeFileSync(join(directory, 'curl'), `#!${process.execPath}
const fs = require('node:fs')
const args = process.argv.slice(2)
const headers = args.includes('@-') ? fs.readFileSync(0, 'utf8') : ''
fs.writeFileSync(process.env.REQUEST_LOG, JSON.stringify({ args, headers }))
if (process.env.RATE_LIMIT === '1' && !headers.includes('Authorization: Bearer ${token}')) {
  console.error('curl: (22) The requested URL returned error: 403')
  process.exit(22)
}
if (process.env.DOWNLOAD_FAIL === '1') process.exit(22)
fs.writeFileSync(args[args.indexOf('--output') + 1], process.env.TOOL_BYTES)
`, { mode: 0o755 })
  return {
    destination,
    request: () => JSON.parse(readFileSync(log, 'utf8')),
    requested: () => existsSync(log),
    run: (env = {}, url = api) => spawnSync('bash', ['-c', `set -euo pipefail\n${fetchTool}\nfetch_tool "$@"`, 'download-test', url, digest, destination], {
      encoding: 'utf8',
      env: {
        ...process.env, GH_TOKEN: '', GITHUB_TOKEN: '',
        PATH: `${directory}:${process.env.PATH}`, REQUEST_LOG: log,
        RATE_LIMIT: '0', DOWNLOAD_FAIL: '0', TOOL_BYTES: payload, ...env,
      },
    }),
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  }
}

describe.skipIf(process.platform === 'win32')('Linux build downloads', () => {
  it.each(['GH_TOKEN', 'GITHUB_TOKEN'])('uses %s to download through the anonymous API rate limit', (variable) => {
    const f = fixture()
    try {
      const blocked = f.run({ RATE_LIMIT: '1' })
      expect(blocked.status).toBe(22)
      expect(blocked.stderr).toContain('403')
      expect(existsSync(f.destination)).toBe(false)
      const result = f.run({ RATE_LIMIT: '1', [variable]: token })
      expect(result.status, result.stderr).toBe(0)
      expect(readFileSync(f.destination, 'utf8')).toBe(payload)
      expect(statSync(f.destination).mode & 0o111).toBe(0o111)
      const { args, headers } = f.request()
      expect(headers).toContain('Accept: application/octet-stream')
      expect(headers).toContain(`Authorization: Bearer ${token}`)
      expect(args).toContain(api)
      expect(args).toContain('--location')
      expect(args).not.toContain('--location-trusted')
      expect(JSON.stringify(args)).not.toContain(token)
      expect(result.stdout + result.stderr).not.toContain(token)
    } finally { f.cleanup() }
  })

  it.each([
    'https://raw.githubusercontent.com/tauri-apps/linuxdeploy-plugin-gtk/commit/linuxdeploy-plugin-gtk.sh',
    'https://api.github.com.example.com/tool',
    'https://api.github.com@other.example/tool',
  ])('does not send the token to %s', (url) => {
    const f = fixture()
    try {
      const result = f.run({ GH_TOKEN: token }, url)
      expect(result.status, result.stderr).toBe(0)
      expect(f.request().headers).not.toContain('Authorization')
      expect(JSON.stringify(f.request())).not.toContain(token)
    } finally { f.cleanup() }
  })

  it('supports public downloads without a token', () => {
    const f = fixture()
    try {
      const result = f.run()
      expect(result.status, result.stderr).toBe(0)
      expect(f.request().headers).not.toContain('Authorization')
    } finally { f.cleanup() }
  })

  it('reuses only a cache entry with the pinned digest', () => {
    const f = fixture()
    try {
      writeFileSync(f.destination, payload)
      expect(f.run({ DOWNLOAD_FAIL: '1' }).status).toBe(0)
      expect(f.requested()).toBe(false)
      writeFileSync(f.destination, 'corrupt cache')
      const result = f.run({ GH_TOKEN: token })
      expect(result.status, result.stderr).toBe(0)
      expect(f.requested()).toBe(true)
      expect(readFileSync(f.destination, 'utf8')).toBe(payload)
    } finally { f.cleanup() }
  })

  it.each([{ TOOL_BYTES: 'wrong bytes' }, { DOWNLOAD_FAIL: '1' }])('rejects a failed download without replacing the cache: %j', (env) => {
    const f = fixture()
    try {
      writeFileSync(f.destination, 'old cache')
      const result = f.run({ GH_TOKEN: token, ...env })
      expect(result.status).not.toBe(0)
      expect(readFileSync(f.destination, 'utf8')).toBe('old cache')
      expect(result.stdout + result.stderr).not.toContain(token)
    } finally { f.cleanup() }
  })
})
