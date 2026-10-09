import { it, expect } from 'vitest'
import { existsSync, mkdtempSync, mkdirSync, writeFileSync, copyFileSync, readFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { execFileSync, spawnSync } from 'node:child_process'
import { BUNDLED_LOCALES } from '../scripts/chromium-locales.mjs'

it.skipIf(process.platform !== 'darwin')('packages both CEF architectures, snapshots, bridge and helper loader paths', () => {
  const root = mkdtempSync(join(tmpdir(), 'muniment-cef-package-'))
  const script = resolve('scripts/package-cef-macos.mjs')
  const framework = 'Chromium Embedded Framework.framework'
  const target = join(root, 'src-tauri/target')
  const app = join(root, 'muniment.app')
  const universal = join(target, 'universal-apple-darwin/release')
  const binaries = []
  try {
    writeFileSync(join(root, 'fixture.c'), 'int main(void) { return 0; }\n')
    for (const [arch, triple, cefArch] of [['arm64', 'aarch64-apple-darwin', 'aarch64'], ['x86_64', 'x86_64-apple-darwin', 'x86_64']]) {
      const release = join(target, triple, 'release')
      const source = join(release, 'build/cef-dll-sys-fixture/out', `cef_macos_${cefArch}`)
      for (const directory of ['include', `${framework}/Libraries`, `${framework}/Resources`]) mkdirSync(join(source, directory), { recursive: true })
      writeFileSync(join(source, 'include/cef_version.h'), '#define CEF_VERSION "154.0.34"\n')
      writeFileSync(join(source, 'CREDITS.html'), 'Fixture credits')
      writeFileSync(join(source, framework, 'Resources', `v8_context_snapshot.${arch}.bin`), arch)
      for (const code of [...BUNDLED_LOCALES, 'nl']) {
        const folder = join(source, framework, 'Resources', `${code === 'en-US' ? 'en' : code.replace('-', '_')}.lproj`)
        mkdirSync(folder)
        writeFileSync(join(folder, 'locale.pak'), code)
      }
      const binary = join(release, 'fixture')
      execFileSync('clang', ['-arch', arch, '-dynamiclib', '-Wl,-headerpad_max_install_names', join(root, 'fixture.c'), '-o', binary])
      binaries.push(binary)
      for (const file of ['Chromium Embedded Framework', 'Libraries/libcef_sandbox.dylib', 'Libraries/libvk_swiftshader.dylib', 'Libraries/libvulkan.dylib']) copyFileSync(binary, join(source, framework, file))
      copyFileSync(binary, join(release, 'libmuniment_cef_keychain.dylib'))
    }
    mkdirSync(universal, { recursive: true })
    execFileSync('lipo', ['-create', ...binaries, '-output', join(universal, 'muniment-cef-helper')])
    mkdirSync(join(app, 'Contents/MacOS'), { recursive: true })
    mkdirSync(join(app, 'Contents/Resources'), { recursive: true })
    for (const file of ['muniment-desktop', 'muniment-cef-helper']) copyFileSync(join(universal, 'muniment-cef-helper'), join(app, 'Contents/MacOS', file))
    execFileSync(process.execPath, [script, app, '--universal'], { cwd: root })
    const frameworks = join(app, 'Contents/Frameworks')
    for (const file of [`${framework}/Chromium Embedded Framework`, `${framework}/Libraries/libvulkan.dylib`, 'libmuniment_cef_keychain.dylib']) {
      expect(execFileSync('lipo', ['-archs', join(frameworks, file)], { encoding: 'utf8' })).toMatch(/x86_64.*arm64|arm64.*x86_64/)
    }
    for (const arch of ['arm64', 'x86_64']) expect(readFileSync(join(frameworks, framework, 'Resources', `v8_context_snapshot.${arch}.bin`), 'utf8')).toBe(arch)
    expect(readFileSync(join(frameworks, framework, 'Resources/pt_BR.lproj/locale.pak'), 'utf8')).toBe('pt-BR')
    expect(existsSync(join(frameworks, framework, 'Resources/nl.lproj'))).toBe(false)
    expect(JSON.parse(readFileSync(join(app, 'Contents/Resources/chromium-locales.json'), 'utf8')).available).toEqual(['nl'])
    expect(execFileSync('tar', ['-tzf', join(universal, 'bundle/locales/muniment-chromium-locales.tar.gz')], { encoding: 'utf8' })).toContain('nl.lproj/locale.pak')
    expect(execFileSync('otool', ['-l', join(app, 'Contents/MacOS/muniment-desktop')], { encoding: 'utf8' })).toContain('@executable_path/../Frameworks')
    for (const suffix of ['', ' (GPU)', ' (Renderer)', ' (Plugin)', ' (Alerts)']) {
      const helper = join(frameworks, `muniment CEF Helper${suffix}.app/Contents/MacOS`, `muniment CEF Helper${suffix}`)
      // otool-classic parses a trailing parenthesis as an archive member name.
      const inspection = join(root, 'helper-inspection')
      copyFileSync(helper, inspection)
      expect(execFileSync('otool', ['-l', inspection], { encoding: 'utf8' })).toContain('@executable_path/../../..')
    }
    rmSync(join(target, 'x86_64-apple-darwin/release/build'), { recursive: true })
    const missing = spawnSync(process.execPath, [script, app, '--universal'], { cwd: root, encoding: 'utf8' })
    expect(missing.status).not.toBe(0)
    expect(missing.stderr).toContain('framework is missing for x86_64')
  } finally { rmSync(root, { recursive: true, force: true }) }
}, 30000)
