import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const configPath = 'src-tauri/tauri.conf.json'
const resolverPath = 'src-tauri/runtime/src/directories.rs'
const runnerPath = 'test/e2e/runner/linux.sh'
const postInstallPath = 'src-tauri/packaging/deb/postinst'

const config = JSON.parse(readFileSync(configPath, 'utf8'))
const resolver = readFileSync(resolverPath, 'utf8')
const runner = readFileSync(runnerPath, 'utf8')
const postInstall = readFileSync(postInstallPath, 'utf8')
const resolverFunction = resolver.match(
  /pub fn installed_desktop_executable_from[\s\S]*?\n}\n/,
)?.[0]

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
