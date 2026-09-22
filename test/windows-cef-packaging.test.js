import { it, expect } from 'vitest'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const script = resolve('scripts/package-cef-windows.mjs')
function fixture() {
  const root = mkdtempSync(join(tmpdir(), 'windows-cef-package-'))
  const target = join(root, 'src-tauri/target/release')
  const put = (path, content = path) => {
    mkdirSync(join(root, path, '..'), { recursive: true })
    writeFileSync(join(root, path), content)
  }
  for (const name of ['bootstrap.exe', 'muniment_desktop.dll', 'muniment-desktop.exe', 'libcef.dll', 'chrome_elf.dll', 'icudtl.dat', 'v8_context_snapshot.bin', 'resources.pak', 'locales/en-US.pak', 'muniment-runtime.exe', 'muniment-reader.exe']) put(`src-tauri/target/release/${name}`, name)
  put('src-tauri/target/release/build/cef-dll-sys-fixture/out/cef/CREDITS.html', 'CEF credits')
  put('src-tauri/third-party/sherpa-onnx-v1.13.2/windows-x86_64/onnxruntime.dll', 'speech')
  for (const name of ['LICENSE.md', 'THIRD_PARTY_NOTICES.md', 'THIRD_PARTY_RUST_NOTICES.md']) put(name)
  put('src-tauri/tauri.windows.conf.json', JSON.stringify({ bundle: { resources: {
    'third-party/sherpa-onnx-v1.13.2/windows-x86_64/onnxruntime.dll': 'onnxruntime.dll',
    'target/release/muniment-runtime.exe': 'muniment-runtime.exe',
  } } }))
  const output = join(target, 'cef-app')
  const config = join(root, 'cef-bundle.json')
  const run = installer => spawnSync(process.execPath, [script, output, ...(installer ? ['--installer-config', config] : [])], { cwd: root, encoding: 'utf8' })
  return { root, target, output, config, run }
}

it('bundles the sandbox bootstrap, application DLL, browser data and credits without duplicating existing resources', () => {
  const f = fixture()
  try {
    const result = f.run(true)
    expect(result.status, result.stderr).toBe(0)
    expect(readFileSync(join(f.target, 'muniment-desktop.exe'), 'utf8')).toBe('bootstrap.exe')
    expect(readFileSync(join(f.output, 'muniment-desktop.dll'), 'utf8')).toBe('muniment_desktop.dll')
    expect(existsSync(join(f.output, 'muniment_desktop.dll'))).toBe(false)
    const resources = JSON.parse(readFileSync(f.config, 'utf8')).bundle.resources
    expect(resources['target/release/muniment_desktop.dll']).toBe('muniment-desktop.dll')
    expect(Object.values(resources).filter(name => name === 'muniment-desktop.dll')).toHaveLength(1)
    for (const name of ['libcef.dll', 'chrome_elf.dll', 'icudtl.dat', 'v8_context_snapshot.bin', 'resources.pak', 'CEF-CREDITS.html', 'muniment-desktop.exe.manifest']) {
      expect(resources[`target/release/cef-app/${name}`]).toBe(name)
      expect(existsSync(join(f.root, 'src-tauri', `target/release/cef-app/${name}`))).toBe(true)
    }
    expect(resources['target/release/cef-app/locales/']).toBe('locales/')
    expect(readFileSync(join(f.output, 'locales/en-US.pak'), 'utf8')).toBe('locales/en-US.pak')
    expect(Object.values(resources)).not.toContain('onnxruntime.dll')
    expect(Object.values(resources)).not.toContain('muniment-runtime.exe')
  } finally { rmSync(f.root, { recursive: true, force: true }) }
})

it.each(['bootstrap.exe', 'muniment_desktop.dll', 'libcef.dll', 'icudtl.dat', 'v8_context_snapshot.bin', 'locales'])('rejects a missing %s before creating an installer layout', name => {
  const f = fixture()
  try {
    rmSync(join(f.target, name), { recursive: true })
    const result = f.run(true)
    expect(result.status).not.toBe(0)
    expect(result.stderr).toContain(`Missing CEF Windows resource: ${name}`)
    expect(existsSync(f.config)).toBe(false)
    expect(readFileSync(join(f.target, 'muniment-desktop.exe'), 'utf8')).toBe('muniment-desktop.exe')
  } finally { rmSync(f.root, { recursive: true, force: true }) }
})

it('keeps the standalone CEF test package separate from the Rust executable', () => {
  const f = fixture()
  try {
    const result = f.run(false)
    expect(result.status, result.stderr).toBe(0)
    expect(readFileSync(join(f.output, 'muniment-desktop.exe'), 'utf8')).toBe('bootstrap.exe')
    expect(readFileSync(join(f.target, 'muniment-desktop.exe'), 'utf8')).toBe('muniment-desktop.exe')
    expect(existsSync(f.config)).toBe(false)
  } finally { rmSync(f.root, { recursive: true, force: true }) }
})

it('builds the DLL before staging and uses bundle-only passes for all three MSI variants', () => {
  const build = readFileSync('.github/build-windows-installers.mjs', 'utf8')
  expect(build.match(/run\("build"/g)).toHaveLength(1)
  expect(build).toContain('"--lib", "--features", "tauri/custom-protocol"')
  const staging = build.indexOf('"scripts/package-cef-windows.mjs"')
  expect(staging).toBeGreaterThan(build.indexOf('const libraryBuild'))
  for (const pass of ['per-user installer', 'machine upgrade-base MSI', 'machine MSI']) {
    const line = build.split('\n').find(line => line.startsWith(`run("bundle", "${pass}"`))
    expect(line).toContain('"--config", cefConfig')
    expect(build.indexOf(line)).toBeGreaterThan(staging)
    if (pass !== 'machine upgrade-base MSI') expect(line).toContain('...signArgs')
  }
})
