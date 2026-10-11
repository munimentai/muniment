import { it, expect } from 'vitest'
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { BUNDLED_LOCALES } from '../scripts/chromium-locales.mjs'

const script = resolve('scripts/stage-cef-linux.mjs')
it('stages CEF resources and credits and rejects an incomplete runtime', () => {
  const directory = mkdtempSync(join(tmpdir(), 'cef-package-'))
  try {
    const target = join(directory, 'src-tauri/target/release')
    const source = join(target, 'build/cef-dll-sys-fixture/out/cef_linux_x86_64')
    mkdirSync(source, { recursive: true })
    mkdirSync(join(target, 'locales'))
    writeFileSync(join(source, 'CREDITS.html'), 'credits')
    for (const code of [...BUNDLED_LOCALES, 'nl']) {
      for (const suffix of ['', '_FEMININE']) writeFileSync(join(target, `locales/${code}${suffix}.pak`), 'locale')
    }
    const payload = ['libcef.so', 'icudtl.dat', 'chrome-sandbox', 'muniment-cef-helper', 'resources.pak']
    for (const name of payload) writeFileSync(join(target, name), name)
    writeFileSync(join(target, 'libonnxruntime.so'), 'speech')
    const stripped = join(directory, 'stripped')
    const strip = join(directory, 'strip.sh')
    writeFileSync(strip, `#!/bin/sh\necho "$2" >> '${stripped}'\n`, { mode: 0o755 })
    const run = () => spawnSync(process.execPath, [script], { cwd: directory, encoding: 'utf8', env: { ...process.env, STRIP: strip } })
    const result = run()
    expect(result.status, result.stderr).toBe(0)
    for (const name of payload) expect(readFileSync(join(target, 'cef-resources', name), 'utf8')).toBe(name)
    expect(readFileSync(stripped, 'utf8').trim().split('\n')).toEqual([join('src-tauri/target/release', 'libcef.so')])
    expect(existsSync(join(target, 'cef-resources/libonnxruntime.so'))).toBe(false)
    expect(readFileSync(join(target, 'cef-resources/locales/en-US.pak'), 'utf8')).toBe('locale')
    expect(existsSync(join(target, 'cef-resources/locales/en-US_FEMININE.pak'))).toBe(true)
    expect(existsSync(join(target, 'cef-resources/locales/nl.pak'))).toBe(false)
    const manifest = JSON.parse(readFileSync(join(target, 'cef-resources/chromium-locales.json'), 'utf8'))
    expect(manifest.available).toEqual(['nl'])
    expect(manifest.size).toBeGreaterThan(0)
    const listing = spawnSync('tar', ['-tzf', join(target, 'bundle/locales/muniment-chromium-locales.tar.gz')], { encoding: 'utf8' })
    expect(listing.stdout.trim().split('\n').sort()).toEqual(['nl.pak', 'nl_FEMININE.pak'])
    expect(readFileSync(join(target, 'cef-resources/CEF-CREDITS.html'), 'utf8')).toBe('credits')
    rmSync(join(target, 'libcef.so'))
    const missing = run()
    expect(missing.status).not.toBe(0)
    expect(missing.stderr).toContain('Missing CEF runtime file: libcef.so')
    expect(existsSync(join(target, 'cef-resources/libcef.so'))).toBe(false)
  } finally {
    rmSync(directory, { recursive: true, force: true })
  }
})
