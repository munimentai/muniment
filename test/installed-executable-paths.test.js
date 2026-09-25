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
const userTemplatePath = 'src-tauri/windows/per-user.wxs'
const machineConfigPath = 'src-tauri/tauri.machine.conf.json'
const machineTemplatePath = 'src-tauri/windows/per-machine.wxs'
const windowsBuildPath = '.github/build-windows-installers.mjs'
const windowsInstallerTestPath = 'test/windows-installers.ps1'
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
const userTemplate = readFileSync(userTemplatePath, 'utf8')
const machineConfig = JSON.parse(readFileSync(machineConfigPath, 'utf8'))
const machineTemplate = readFileSync(machineTemplatePath, 'utf8')
const windowsBuild = readFileSync(windowsBuildPath, 'utf8')
const windowsInstallerTest = readFileSync(windowsInstallerTestPath, 'utf8')
const ci = readFileSync(ciPath, 'utf8')
const resolverFunction = resolver.match(
  /pub fn installed_desktop_executable_from[\s\S]*?\n}\n/,
)?.[0]

function plistValue(key) {
  const match = launchAgent.match(new RegExp(`<key>${key}</key>\\s*<string>([^<]*)</string>`))
  return match?.[1]
}

describe('installed desktop executable paths', () => {
  it('admits the packaged desktop executable without the deb alias', () => {
    const installedName = resolverFunction?.match(/\.join\("bin\/([^"/]+)"\)/)?.[1]
    const manifest = readFileSync('src-tauri/Cargo.toml', 'utf8')
    const binaryName = manifest.match(/^name = "([^"]+)"/m)?.[1]

    expect(binaryName).toBe('muniment-desktop')
    expect(installedName).toBe(binaryName)
  })

  it('uses the installed runtime path that the Linux runner probes', () => {
    const resolverSegments = [...(resolverFunction?.matchAll(/OsStr::new\("([^"]+)"\)/g) ?? [])]
      .map((match) => match[1])
    const installedRuntime = runner.match(
      /\[\[ -x (\/\S+) \]\] \|\| \{ runner_failure 'installed runtime is unavailable or not executable'/,
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

  it('installs the trusted payload without replacing the product-name alias', () => {
    const installedDesktop = runner.match(/installed_desktop=(\/\S+)/)?.[1]
    const installedName = resolverFunction?.match(/\.join\("bin\/([^"/]+)"\)/)?.[1]

    expect(installedName).toBe('muniment-desktop')
    expect(installedDesktop).toBe(`/usr/bin/${installedName}`)
    expect(installedDesktop).not.toBe(`/usr/bin/${config.productName}`)
    expect(postInstall).toContain(`ln -sfn ${installedName} /usr/bin/${config.productName}`)
    expect(runner).toContain('[[ -f $installed_desktop ]]')
    expect(runner).toContain('app_binary=$installed_desktop')
  })
})

describe('Windows runtime bundle paths', () => {
  it('The release runtime uses the Windows subsystem without a console window.', () => {
    const main = readFileSync('src-tauri/runtime/src/main.rs', 'utf8')
    expect(main).toMatch(/^#!\[cfg_attr\(not\(debug_assertions\), windows_subsystem = "windows"\)\]/)
  })

  const runtimeResource = {
    'target/release/muniment-runtime.exe': 'muniment-runtime.exe',
  }

  it('configures the regular MSI for the per-user install root', () => {
    expect(windowsConfig.bundle.resources).toMatchObject(runtimeResource)
    expect(windowsConfig.bundle.windows.wix.template).toBe('./windows/per-user.wxs')
    expect(userTemplate).toContain('InstallScope="perUser"')
    expect(userTemplate).not.toContain('<Directory Id="LocalAppDataFolder">')
    expect(userTemplate).toMatch(/<Directory Id="TARGETDIR" Name="SourceDir">\s*<Directory Id="INSTALLDIR" Name="{{product_name}}"\/>/)
    expect(userTemplate).toContain('<SetDirectory Id="INSTALLDIR" Value="[LocalAppDataFolder]{{product_name}}" Sequence="both">')
    expect(userTemplate).toContain('{{resources}}')
  })

  it('configures the machine MSI for the Program Files install root', () => {
    expect(machineConfig.bundle.resources).toMatchObject(runtimeResource)
    expect(machineTemplate).toContain('<Directory Id="$(var.PlatformProgramFilesFolder)" Name="PFiles">')
    expect(machineTemplate).toContain('<Directory Id="INSTALLDIR" Name="{{product_name}}"/>')
    expect(machineTemplate).toContain('{{resources}}')
  })

  it('checks each installed runtime path', () => {
    expect(windowsInstallerTest).toContain('Join-Path $env:LOCALAPPDATA "muniment\\muniment-runtime.exe"')
    expect(windowsInstallerTest).toContain('throw "NSIS runtime not found at $userRuntime"')
    expect(windowsInstallerTest).toContain('throw "Regular MSI runtime not found at $userRuntime"')
    expect(windowsInstallerTest).toContain('Join-Path $env:ProgramFiles "muniment\\muniment-runtime.exe"')
    expect(windowsInstallerTest).toContain('throw "Machine MSI runtime not found at $machineRuntime"')
  })

  // Signing waits for the signing phase, after the unsigned upgrade-base
  // fixture, and precedes every installer pass that ships.
  it('builds the runtime and signs it before the first signed installer pass', () => {
    const runtimeBuild = windowsBuild.indexOf('"--package", "muniment-runtime"')
    const runtimeSigning = windowsBuild.indexOf('signFile(runtime)')
    const signedInstaller = windowsBuild.indexOf('run("bundle", "per-user installer"')

    expect(runtimeBuild).toBeGreaterThan(-1)
    expect(runtimeSigning).toBeGreaterThan(runtimeBuild)
    expect(signedInstaller).toBeGreaterThan(runtimeSigning)
    expect(windowsBuild.indexOf('run("bundle", "machine MSI"')).toBeGreaterThan(runtimeSigning)
  })
})

describe('macOS runtime bundle paths', () => {
  it('places the runtime, the record server, the reader and the LaunchAgent in the bundle Library', () => {
    expect(macosConfig.bundle.macOS.files).toEqual({
      'Library/LaunchServices/muniment-runtime': 'target/universal-apple-darwin/release/muniment-runtime',
      'Library/LaunchServices/muniment-cli': 'target/universal-apple-darwin/release/muniment-cli',
      'Library/LaunchServices/muniment-reader': 'target/universal-apple-darwin/release/muniment-reader',
      'Library/LaunchAgents/ai.muniment.runtime.plist': 'packaging/ai.muniment.runtime.plist',
    })
  })

  it('stages and checks the universal runtime before macOS bundle builds', () => {
    expect(macosBuild).toContain('["x86_64-apple-darwin", "aarch64-apple-darwin"]')
    expect(macosBuild).toContain('mustRun("lipo"')
    expect(macosBuild).toContain('"--package", "muniment-cli"')
    expect(macosBuild).toContain('if (!existsSync(output))')
    expect(macosAppBuild.indexOf('build-macos-runtime.mjs'))
      .toBeLessThan(macosAppBuild.indexOf('tauri("build"'))
    expect(ci).toContain('MACOS_SIGNING_ENABLED=false node .github/build-macos-app.mjs')
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
