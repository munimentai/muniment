import { readFileSync } from 'node:fs'
import { isAbsolute } from 'node:path'
import { describe, expect, it } from 'vitest'

const configPath = 'src-tauri/tauri.conf.json'
const resolverPath = 'src-tauri/runtime/src/directories.rs'
const runnerPath = 'test/e2e/runner/linux.sh'
const postInstallPath = 'src-tauri/packaging/deb/postinst'
const macosConfigPath = 'src-tauri/tauri.macos.conf.json'
const launchAgentPath = 'src-tauri/packaging/ai.muniment.runtime.plist'
const macosBuildPath = '.github/build-macos-runtime.mjs'
const macosAppBuildPath = '.github/build-macos-app.mjs'
const windowsConfigPath = 'src-tauri/tauri.windows.conf.json'
const machineConfigPath = 'src-tauri/tauri.machine.conf.json'
const machineTemplatePath = 'src-tauri/windows/per-machine.wxs'
const windowsBuildPath = '.github/build-windows-installers.mjs'
const ciPath = '.github/workflows/ci.yml'

const config = JSON.parse(readFileSync(configPath, 'utf8'))
const resolver = readFileSync(resolverPath, 'utf8')
const runner = readFileSync(runnerPath, 'utf8')
const postInstall = readFileSync(postInstallPath, 'utf8')
const macosConfig = JSON.parse(readFileSync(macosConfigPath, 'utf8'))
const launchAgent = readFileSync(launchAgentPath, 'utf8')
const macosBuild = readFileSync(macosBuildPath, 'utf8')
const macosAppBuild = readFileSync(macosAppBuildPath, 'utf8')
const windowsConfig = JSON.parse(readFileSync(windowsConfigPath, 'utf8'))
const machineConfig = JSON.parse(readFileSync(machineConfigPath, 'utf8'))
const machineTemplate = readFileSync(machineTemplatePath, 'utf8')
const windowsBuild = readFileSync(windowsBuildPath, 'utf8')
const ci = readFileSync(ciPath, 'utf8')
const resolverFunction = resolver.match(
  /pub fn installed_desktop_executable_from[\s\S]*?\n}\n/,
)?.[0]

function plistValue(key) {
  const match = launchAgent.match(new RegExp(`<key>${key}</key>\\s*<string>([^<]*)</string>`))
  return match?.[1]
}

describe('installed desktop executable paths', () => {
  it('uses the configured product name for the installed desktop executable', () => {
    const installedName = resolverFunction?.match(/\.join\("bin\/([^"/]+)"\)/)?.[1]

    expect(installedName, `${resolverPath} bin value ${installedName} disagrees with ${configPath} productName ${config.productName}`)
      .toBe(config.productName)
  })

  it('uses the installed runtime path that the Linux runner probes', () => {
    const resolverSegments = [...(resolverFunction?.matchAll(/OsStr::new\("([^"]+)"\)/g) ?? [])]
      .map((match) => match[1])
    const installedRuntime = runner.match(
      /\[\[ -x (\/\S+) \]\] \|\| \{ echo 'installed runtime is unavailable or not executable'/,
    )?.[1]
    const runnerSegments = installedRuntime?.split('/').filter(Boolean)
    const runnerLibrary = runnerSegments?.at(-3)
    const runnerRuntime = runnerSegments?.at(-1)

    expect(installedRuntime, `${runnerPath} installed runtime value ${installedRuntime} disagrees with ${resolverPath} runtime path`)
      .toBeTruthy()
    expect(resolverSegments[2], `${resolverPath} lib value ${resolverSegments[2]} disagrees with ${runnerPath} value ${runnerLibrary}`)
      .toBe(runnerLibrary)
    expect(resolverSegments[0], `${resolverPath} runtime value ${resolverSegments[0]} disagrees with ${runnerPath} value ${runnerRuntime}`)
      .toBe(runnerRuntime)
  })

  it('installs the product-name desktop path that the Linux runner remaps', () => {
    const installedDesktop = runner.match(/installed_desktop=(\/\S+)/)?.[1]

    expect(installedDesktop, `${runnerPath} installed desktop value ${installedDesktop} disagrees with ${configPath} productName ${config.productName}`)
      .toBe(`/usr/bin/${config.productName}`)
    expect(postInstall, `${postInstallPath} does not install ${installedDesktop}`)
      .toContain(`ln -sfn muniment-desktop ${installedDesktop}`)
    expect(runner).toContain('[[ -f $installed_desktop ]]')
  })
})

describe('Windows runtime bundle paths', () => {
  const runtimeResource = {
    'target/release/muniment-runtime.exe': 'muniment-runtime.exe',
  }

  it('places the NSIS and regular MSI runtime at the per-user install root', () => {
    expect(windowsConfig.bundle.resources).toMatchObject(runtimeResource)
  })

  it('places the machine MSI runtime under Program Files', () => {
    expect(machineConfig.bundle.resources).toMatchObject(runtimeResource)
    expect(machineTemplate).toContain('<Directory Id="$(var.PlatformProgramFilesFolder)" Name="PFiles">')
    expect(machineTemplate).toContain('<Directory Id="INSTALLDIR" Name="{{product_name}}"/>')
    expect(machineTemplate).toContain('{{resources}}')
  })

  it('builds and signs the runtime before the first installer pass', () => {
    const runtimeBuild = windowsBuild.indexOf('"--package", "muniment-runtime"')
    const runtimeSigning = windowsBuild.indexOf('signFile(runtime)')
    const installerBuild = windowsBuild.indexOf('run("build"')

    expect(runtimeBuild).toBeGreaterThan(-1)
    expect(runtimeSigning).toBeGreaterThan(runtimeBuild)
    expect(installerBuild).toBeGreaterThan(runtimeSigning)
  })
})

describe('macOS runtime bundle paths', () => {
  it('places the runtime and LaunchAgent in the bundle Library', () => {
    expect(macosConfig.bundle.macOS.files).toEqual({
      'Library/LaunchServices/muniment-runtime': 'target/universal-apple-darwin/release/muniment-runtime',
      'Library/LaunchAgents/ai.muniment.runtime.plist': 'packaging/ai.muniment.runtime.plist',
    })
  })

  it('stages and checks the universal runtime before macOS bundle builds', () => {
    expect(macosBuild).toContain('["x86_64-apple-darwin", "aarch64-apple-darwin"]')
    expect(macosBuild).toContain('mustRun("lipo"')
    expect(macosBuild).toContain('if (!existsSync(runtime))')
    expect(macosAppBuild.indexOf('build-macos-runtime.mjs'))
      .toBeLessThan(macosAppBuild.indexOf('tauri("build"'))
    expect(ci).toContain('node .github/build-macos-runtime.mjs && npm run tauri build -- --target universal-apple-darwin')
  })

  it('signs the bundled runtime before the app', () => {
    const runtimeSigning = macosAppBuild.indexOf('mustRun("codesign runtime"')
    const appSigning = macosAppBuild.indexOf('mustRun("codesign app"')

    expect(runtimeSigning).toBeGreaterThan(-1)
    expect(runtimeSigning).toBeLessThan(appSigning)
  })

  it('defines the bundled runtime LaunchAgent', () => {
    expect(plistValue('Label')).toBe('ai.muniment.runtime')
    expect(plistValue('BundleProgram')).toBe('Contents/Library/LaunchServices/muniment-runtime')
    expect(isAbsolute(plistValue('BundleProgram'))).toBe(false)
    expect(launchAgent).not.toMatch(/<key>Program(?:Arguments)?<\/key>/)
    expect(launchAgent).toMatch(/<key>KeepAlive<\/key>\s*<dict>\s*<key>SuccessfulExit<\/key>\s*<false\/>\s*<\/dict>/)
    expect(launchAgent).toMatch(/<key>ThrottleInterval<\/key>\s*<integer>5<\/integer>/)
    expect(plistValue('StandardOutPath')).toBe('/dev/null')
    expect(plistValue('StandardErrorPath')).toBe('/dev/null')
  })
})
