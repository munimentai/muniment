// @vitest-environment node
import { afterEach, describe, expect, it, vi } from 'vitest'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { chooseFolder, folderDialogDescription } from './e2e/support/onboarding-folder.mjs'

const title = '(Select|Open|Choose|Pick).*([Ff]older|[Dd]irectory|[Ff]ile)'
const temporary = []
async function rawDirectory() {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'muniment-folder-test-'))
  temporary.push(directory)
  return directory
}
afterEach(async () => {
  vi.useRealTimers()
  await Promise.all(temporary.splice(0).map((directory) => rm(directory, { recursive: true, force: true })))
})

const nativeCases = [
  ['darwin', '/tmp/Home space "quote" café', '/usr/bin/osascript', 'folder-dialog-macos.applescript', 'NSOpenPanel'],
  ['win32', String.raw`C:\Users\Test\Home space & café`, 'powershell.exe', 'folder-dialog-windows.ps1', '#32770'],
]

describe('The onboarding folder picker selects a native driver.', () => {
  it.each(nativeCases)('The picker uses only the native driver on %s.', async (platform, home, binary, script) => {
    const execute = vi.fn().mockResolvedValue({ stdout: '' })
    await chooseFolder(home, 30, title, await rawDirectory(), execute, platform)
    expect(execute).toHaveBeenCalledTimes(1)
    const [command, args, options] = execute.mock.calls[0]
    expect(command.endsWith(binary)).toBe(true)
    expect(args.some((arg) => arg.endsWith(script))).toBe(true)
    expect(options.timeout).toBe(35000)
    expect([command, ...args].join(' ')).not.toMatch(/xdotool|\btimeout\b/)
    if (platform === 'darwin') {
      expect(args.slice(1)).toEqual([home, '30'])
    } else {
      expect(args).toHaveLength(6)
      expect(args.slice(0, 5)).toEqual(['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File'])
      expect(options.env.MUNIMENT_FOLDER_PATH).toBe(home)
      expect(options.env.MUNIMENT_FOLDER_WAIT_SECONDS).toBe('30')
      expect(args).not.toContain(home)
    }
  })

  it('The Linux picker drive stays unchanged.', async () => {
    vi.useFakeTimers()
    const execute = vi.fn().mockResolvedValue({ stdout: '101\n202\n' })
    const result = chooseFolder('/tmp/home', 30, title, await rawDirectory(), execute, 'linux')
    await vi.runAllTimersAsync()
    await result
    expect(execute.mock.calls).toEqual([
      ['timeout', ['30', 'xdotool', 'search', '--sync', '--onlyvisible', '--name', title]],
      ['xdotool', ['windowfocus', '--sync', '202']],
      ['xdotool', ['key', '--window', '202', '--clearmodifiers', 'ctrl+l']],
      ['xdotool', ['type', '--window', '202', '--clearmodifiers', '--delay', '1', '/tmp/home']],
      ['xdotool', ['key', '--window', '202', '--clearmodifiers', 'Return']],
      ['xdotool', ['getwindowname', '202']],
      ['xdotool', ['key', '--window', '202', '--clearmodifiers', 'alt+s']],
    ])
  })

  it.each([
    ...nativeCases,
    ['linux', '/tmp/home', 'timeout', '', title],
  ])('A failed drive on %s records the platform and window.', async (platform, home, _binary, _script, window) => {
    const raw = await rawDirectory()
    const cause = Object.assign(new Error('The native driver failed.'), {
      code: 'ENOENT', stdout: 'window: test picker', stderr: 'access denied',
    })
    const execute = vi.fn().mockRejectedValue(cause)
    const description = folderDialogDescription(title, platform)
    await expect(chooseFolder(home, 30, title, raw, execute, platform)).rejects.toMatchObject({
      message: `Home picker failed. ${description}. ${cause.message}`, cause,
    })
    const artifact = await readFile(path.join(raw, 'folder-picker-failure.log'), 'utf8')
    expect(artifact).toContain(description)
    expect(artifact).toContain(window)
    expect(artifact).toContain('window: test picker')
    expect(artifact).toContain('access denied')
  })

  it('A Linux timeout keeps the window diagnostics beside the failure envelope.', async () => {
    const raw = await rawDirectory()
    const execute = vi.fn(async (command, args) => {
      if (command === 'timeout') throw Object.assign(new Error('The window search timed out.'), { code: 124 })
      return { stdout: args[0] === 'search' ? '101\n' : 'muniment\n' }
    })
    await expect(chooseFolder('/tmp/home', 30, title, raw, execute, 'linux')).rejects.toThrow('platform: linux')
    expect(await readFile(path.join(raw, 'folder-picker-timeout.log'), 'utf8')).toContain('101: muniment')
    expect(await readFile(path.join(raw, 'folder-picker-failure.log'), 'utf8')).toContain(title)
  })

  it('An artifact write failure keeps the platform and window in the error.', async () => {
    const cause = new Error('The picker lost focus.')
    const execute = vi.fn().mockRejectedValue(cause)
    const raw = path.join(await rawDirectory(), 'absent')
    await expect(chooseFolder('/tmp/home', 30, title, raw, execute, 'darwin')).rejects.toMatchObject({
      message: `Home picker failed. ${folderDialogDescription(title, 'darwin')}. ${cause.message}`, cause,
    })
  })

  it.each([0, -1, 121, 1.5, NaN, Infinity, '30', undefined])('The picker rejects an invalid wait (%s) before a spawn.', async (wait) => {
    const execute = vi.fn()
    await expect(chooseFolder('/tmp/home', wait, title, await rawDirectory(), execute, 'darwin')).rejects.toThrow('integer from 1 to 120')
    expect(execute).not.toHaveBeenCalled()
  })

  it.each([1, 120])('The picker accepts the wait boundary (%s).', async (wait) => {
    const execute = vi.fn().mockResolvedValue({ stdout: '' })
    await chooseFolder('/tmp/home', wait, title, await rawDirectory(), execute, 'darwin')
    expect(execute.mock.calls[0][2].timeout).toBe((wait + 5) * 1000)
  })

  it.each(['', 'relative', '/tmp/line\nbreak', '/tmp/null\0byte', undefined])('The picker rejects an invalid Home (%s) before a spawn.', async (home) => {
    const execute = vi.fn()
    await expect(chooseFolder(home, 30, title, await rawDirectory(), execute, 'darwin')).rejects.toThrow('absolute Home path')
    expect(execute).not.toHaveBeenCalled()
  })

  it('The Windows picker keeps a UNC path literal.', async () => {
    const home = String.raw`\\server\share\Home space & café`
    const execute = vi.fn().mockResolvedValue({ stdout: '' })
    await chooseFolder(home, 30, title, await rawDirectory(), execute, 'win32')
    expect(execute.mock.calls[0][2].env.MUNIMENT_FOLDER_PATH).toBe(home)
  })

  it('The Windows picker rejects a drive-relative path before a spawn.', async () => {
    const execute = vi.fn()
    await expect(chooseFolder('C:home', 30, title, await rawDirectory(), execute, 'win32')).rejects.toThrow('absolute Home path')
    expect(execute).not.toHaveBeenCalled()
  })

  it('The native drivers scope their window searches and bound each wait.', async () => {
    const macos = await readFile(new URL('./e2e/support/folder-dialog-macos.applescript', import.meta.url), 'utf8')
    expect(macos).toContain('application processes whose bundle identifier is "ai.muniment.desktop"')
    expect(macos).toContain('if (count appProcesses) is not 1 then error')
    expect(macos).toContain('if (count matches) > 1 then error')
    expect(macos).toContain('{appWindow} & (sheets of appWindow)')
    expect(macos).toContain('if (current date) >= deadline then error')
    expect(macos).toContain('set value of pathField to homePath')
    expect(macos).toContain('if not (frontmost of appProcess) then error')
    expect(macos).toContain('repeat while (my findPanel(appProcess)) is not missing value')
    const windows = await readFile(new URL('./e2e/support/folder-dialog-windows.ps1', import.meta.url), 'utf8')
    expect(windows).toContain("Get-Process -Name 'muniment-desktop'")
    expect(windows).toContain('$_.Current.ProcessId -in $processIds')
    expect(windows).toContain("$_.Current.ClassName -eq '#32770' -and -not $_.Current.IsOffscreen")
    expect(windows).toContain('if ($matches.Count -gt 1) { throw')
    expect(windows).toContain('if ([DateTime]::UtcNow -ge $deadline) { throw')
    expect(windows).toContain("AutomationIdProperty, '1152'")
    expect(windows).toContain('[System.Windows.Automation.ControlType]::Edit')
    expect(windows).toContain('$value.SetValue($homePath)')
    expect(windows).toContain('$_.Current.NativeWindowHandle -eq $handle')
    expect(windows).not.toMatch(/SendKeys|SendWait|Invoke-Expression/)
  })

  it('The picker rejects an unsupported platform before a spawn.', async () => {
    const execute = vi.fn()
    await expect(chooseFolder('/tmp/home', 30, title, await rawDirectory(), execute, 'freebsd')).rejects.toThrow('platform: freebsd')
    expect(execute).not.toHaveBeenCalled()
  })

  it('The spec keeps the native picker and Home path assertions.', async () => {
    const spec = await readFile(new URL('./e2e/specs/onboarding.spec.js', import.meta.url), 'utf8')
    expect(spec).toContain("from '../support/onboarding-folder.mjs'")
    expect(spec).toContain('await chooseFolder(')
    expect(spec).toContain('await homePathMatches(location, home)')
    expect(spec).toContain("expect(await location.getProperty('textContent')).toBe(home)")
    expect(spec).toContain('${folderDialogDescription(FOLDER_DIALOG_TITLE)}')
    expect(spec).not.toMatch(/mock|plugin:dialog\|open/)
  })
})
