import { it, expect } from 'vitest'
import { spawnSync } from 'node:child_process'
import { chmodSync, cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { packageAppImage, prepareAppDir, verifyTree } from '../scripts/package-appimage-linux.mjs'

function fixture() {
  const directory = mkdtempSync(join(tmpdir(), 'appimage-test-'))
  const appDir = join(directory, 'muniment.AppDir')
  const lib = join(appDir, 'usr/lib')
  const cef = join(lib, 'muniment/cef')
  mkdirSync(join(cef, 'locales'), { recursive: true })
  for (const name of ['icudtl.dat', 'v8_context_snapshot.bin', 'resources.pak', 'libcef.so']) writeFileSync(join(cef, name), name)
  writeFileSync(join(cef, 'locales/en-US.pak'), 'English')
  writeFileSync(join(appDir, 'AppRun'), 'executable')
  chmodSync(join(appDir, 'AppRun'), 0o755)
  writeFileSync(join(directory, 'muniment.AppImage'), 'unverified original')
  return { directory, appDir, lib, cef }
}

it.skipIf(process.platform === 'win32')('puts Chromium data beside libcef and uses complete host NSS and Wayland stacks', () => {
  const f = fixture()
  try {
    for (const name of ['libnss3.so', 'libnssutil3.so', 'libwayland-client.so.0', 'libwayland-egl.so.1', 'libcrypto.so.3']) writeFileSync(join(f.lib, name), name)
    writeFileSync(join(f.lib, 'libcef.so'), 'linuxdeploy patched library')
    prepareAppDir(f.appDir)
    expect(readlinkSync(join(f.lib, 'chrome-sandbox'))).toBe('/usr/lib/muniment/cef/chrome-sandbox')
    prepareAppDir(f.appDir) // Repackaging preserves the fixed host link.
    expect(readFileSync(join(f.lib, 'libcef.so'), 'utf8')).toBe('linuxdeploy patched library')
    expect(readFileSync(join(f.lib, 'icudtl.dat'), 'utf8')).toBe('icudtl.dat')
    expect(readFileSync(join(f.lib, 'v8_context_snapshot.bin'), 'utf8')).toBe('v8_context_snapshot.bin')
    expect(readFileSync(join(f.lib, 'locales/en-US.pak'), 'utf8')).toBe('English')
    for (const name of ['libnss3.so', 'libnssutil3.so', 'libwayland-client.so.0', 'libwayland-egl.so.1']) expect(existsSync(join(f.lib, name))).toBe(false)
    expect(existsSync(join(f.lib, 'libcrypto.so.3'))).toBe(true)
    rmSync(join(f.cef, 'icudtl.dat'))
    expect(() => prepareAppDir(f.appDir)).toThrow('Missing CEF AppImage resource: icudtl.dat')
  } finally { rmSync(f.directory, { recursive: true, force: true }) }
})

it('rejects altered bytes, missing files, extra files and lost executable modes after extraction', async () => {
  const f = fixture(), extracted = join(f.directory, 'extracted')
  try {
    cpSync(f.appDir, extracted, { recursive: true })
    await verifyTree(f.appDir, extracted)
    writeFileSync(join(extracted, 'AppRun'), 'corrupt data')
    await expect(verifyTree(f.appDir, extracted)).rejects.toThrow('AppImage bytes differ')
    cpSync(join(f.appDir, 'AppRun'), join(extracted, 'AppRun'))
    if (process.platform !== 'win32') {
      chmodSync(join(extracted, 'AppRun'), 0o644)
      await expect(verifyTree(f.appDir, extracted)).rejects.toThrow('AppImage executable mode differs')
      chmodSync(join(extracted, 'AppRun'), 0o755)
    }
    writeFileSync(join(extracted, 'unexpected'), 'extra')
    await expect(verifyTree(f.appDir, extracted)).rejects.toThrow('AppImage entries differ')
    rmSync(join(extracted, 'unexpected'))
    rmSync(join(extracted, 'AppRun'))
    await expect(verifyTree(f.appDir, extracted)).rejects.toThrow('AppImage entries differ')
  } finally { rmSync(f.directory, { recursive: true, force: true }) }
})

it.skipIf(process.platform === 'win32')('rejects a changed symbolic link', async () => {
  const f = fixture(), extracted = join(f.directory, 'extracted')
  try {
    symlinkSync('AppRun', join(f.appDir, 'entry'))
    cpSync(f.appDir, extracted, { recursive: true, verbatimSymlinks: true })
    await verifyTree(f.appDir, extracted)
    rmSync(join(extracted, 'entry'))
    symlinkSync('usr', join(extracted, 'entry'))
    await expect(verifyTree(f.appDir, extracted)).rejects.toThrow('AppImage link differs')
  } finally { rmSync(f.directory, { recursive: true, force: true }) }
})

it.skipIf(process.platform === 'win32')('keeps the original archive when extraction fails and publishes only verified bytes', async () => {
  const f = fixture()
  try {
    let failExtraction = true
    const run = (command, args, options) => {
      if (command === 'bash') {
        expect(args[0]).toMatch(/prepare-appimage-tool-linux\.sh$/)
        expect(args[1]).toBe(f.directory)
        return { status: 0 }
      }
      if (args.includes('--appdir')) {
        expect(options.env.LDAI_OUTPUT).toContain(join(f.directory, '.muniment-appimage-'))
        writeFileSync(options.env.LDAI_OUTPUT, 'verified replacement')
      }
      else if (failExtraction) return { status: 1 }
      else cpSync(f.appDir, join(options.cwd, 'squashfs-root'), { recursive: true, verbatimSymlinks: true })
      return { status: 0 }
    }
    await expect(packageAppImage(f.directory, f.directory, run)).rejects.toThrow('AppImage command failed')
    expect(readFileSync(join(f.directory, 'muniment.AppImage'), 'utf8')).toBe('unverified original')
    failExtraction = false
    await packageAppImage(f.directory, f.directory, run)
    expect(readFileSync(join(f.directory, 'muniment.AppImage'), 'utf8')).toBe('verified replacement')
  } finally { rmSync(f.directory, { recursive: true, force: true }) }
})

it('selects only the exact nightly AppImage and rejects missing or duplicate assets', () => {
  const sha = 'a'.repeat(40)
  const asset = { name: `nightly-${sha}-linux-muniment_0.0.1_amd64.AppImage`, id: 42 }
  const select = assets => spawnSync(process.execPath, ['test/e2e/support/asset-identity.mjs', sha, 'linux-appimage'], {
    input: JSON.stringify({ assets }), encoding: 'utf8',
  })
  expect(select([asset]).stdout).toBe('42')
  for (const assets of [[], [asset, asset], [{ ...asset, name: asset.name + '.sig' }],
    [{ ...asset, name: asset.name.replace(sha, 'b'.repeat(40)) }],
    [{ ...asset, name: asset.name.replace('amd64', 'arm64') }]]) {
    expect(select(assets).status).not.toBe(0)
  }
})

it('skips AppImage tools when a build produces only a DEB', () => {
  const root = mkdtempSync(join(tmpdir(), 'deb-only-test-'))
  try {
    const bundle = join(root, 'src-tauri/target/release/bundle/deb')
    mkdirSync(bundle, { recursive: true })
    writeFileSync(join(bundle, 'muniment.deb'), 'deb fixture')
    const result = spawnSync(process.execPath, [resolve('scripts/package-appimage-linux.mjs')], {
      cwd: root, encoding: 'utf8', env: { ...process.env, PATH: '' },
    })
    expect(result.status, result.stderr).toBe(0)
    expect(readFileSync(join(bundle, 'muniment.deb'), 'utf8')).toBe('deb fixture')
  } finally { rmSync(root, { recursive: true, force: true }) }
})
