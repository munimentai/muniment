// @vitest-environment node
import { afterEach, describe, expect, it, vi } from 'vitest'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import { chooseFolder, folderDialogDescription, withWindowsPickerDiagnostics } from './e2e/support/onboarding-folder.mjs'
import { driveMacosFolder } from './e2e/support/folder-dialog-macos.mjs'

const title = '(Select|Open|Choose|Pick).*([Ff]older|[Dd]irectory|[Ff]ile)'
const temporary = []
async function rawDirectory() {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'muniment-folder-test-'))
  temporary.push(directory)
  return directory
}
afterEach(async () => {
  vi.useRealTimers()
  vi.unstubAllGlobals()
  await Promise.all(temporary.splice(0).map((directory) => rm(directory, { recursive: true, force: true })))
})

const nativeCases = [
  ['darwin', '/tmp/Home space "quote" café', '', '', 'NSOpenPanel'],
  ['win32', String.raw`C:\Users\Test\Home space & café`, 'powershell.exe', 'folder-dialog-windows.ps1', '#32770'],
]

describe('The Windows picker outcome report preserves the failing check.', () => {
  it.each([
    { status: 'pending' },
    { status: 'resolved', value: null },
    { status: 'resolved', value: String.raw`C:\Home` },
    { status: 'rejected', error: 'The dialog call failed.' },
  ])('The report records the open() outcome $status.', async (outcome) => {
    const raw = await rawDirectory()
    const cause = new Error('The shell folder dialog timed out during the window search.')
    await expect(withWindowsPickerDiagnostics(() => Promise.reject(cause), () => JSON.stringify(outcome), raw, 'win32'))
      .rejects.toMatchObject({ message: `${cause.message} Home picker open() outcome: ${JSON.stringify(outcome)}`, cause })
    expect(await readFile(path.join(raw, 'folder-picker-failure.log'), 'utf8')).toContain(JSON.stringify(outcome))
  })

  it('The report names a Home path mismatch after the native drive returns.', async () => {
    const cause = new Error('The Home picker DOM value did not match the isolated Home.')
    const raw = await rawDirectory()
    await expect(withWindowsPickerDiagnostics(() => Promise.reject(cause), () => ({ status: 'resolved', value: 'C:\\wrong' }), raw, 'win32'))
      .rejects.toMatchObject({ cause, message: expect.stringContaining(cause.message) })
  })

  it('The report preserves a failure when the outcome query and artifact write fail.', async () => {
    const cause = new Error('The window search timed out.')
    const raw = path.join(await rawDirectory(), 'absent')
    await expect(withWindowsPickerDiagnostics(() => Promise.reject(cause), () => { throw new Error('The WebView disconnected.') }, raw, 'win32'))
      .rejects.toMatchObject({ cause, message: expect.stringContaining('The WebView disconnected.') })
  })

  it.each([null, undefined, '', 'The dialog call failed.'])('The report preserves a non-Error rejection (%s).', async (cause) => {
    const raw = await rawDirectory()
    await expect(withWindowsPickerDiagnostics(() => Promise.reject(cause), () => Promise.reject(null), raw, 'win32'))
      .rejects.toMatchObject({ cause, message: `${String(cause)} Home picker open() outcome: The picker outcome query failed: null` })
  })

  it('The report names an absent outcome.', async () => {
    const raw = await rawDirectory()
    await withWindowsPickerDiagnostics(() => undefined, () => null, raw, 'win32')
    expect(await readFile(path.join(raw, 'folder-picker-outcome.log'), 'utf8')).toContain('The picker outcome is absent.')
  })

  it('The report records success without changing the result.', async () => {
    const raw = await rawDirectory()
    const outcome = { status: 'resolved', value: 'C:\\Home' }
    expect(await withWindowsPickerDiagnostics(() => 42, () => outcome, raw, 'win32')).toBe(42)
    expect(await readFile(path.join(raw, 'folder-picker-outcome.log'), 'utf8')).toContain(JSON.stringify(outcome))
  })

  it.each(['linux', 'darwin'])('The report leaves the %s drive unchanged.', async (platform) => {
    const cause = new Error('The native drive failed.')
    const readOutcome = vi.fn()
    await expect(withWindowsPickerDiagnostics(() => Promise.reject(cause), readOutcome, await rawDirectory(), platform)).rejects.toBe(cause)
    expect(readOutcome).not.toHaveBeenCalled()
  })
})

describe('The onboarding folder picker selects a native driver.', () => {
  it.each(nativeCases)('The picker uses only the native driver on %s.', async (platform, home, binary, script) => {
    const execute = vi.fn().mockResolvedValue({ stdout: '' })
    const driveMacos = vi.fn().mockResolvedValue(undefined)
    await chooseFolder(home, 30, title, await rawDirectory(), execute, platform, driveMacos)
    if (platform === 'darwin') {
      expect(execute).not.toHaveBeenCalled()
      expect(driveMacos).toHaveBeenCalledExactlyOnceWith(30)
      return
    }
    expect(driveMacos).not.toHaveBeenCalled()
    expect(execute).toHaveBeenCalledTimes(1)
    const [command, args, options] = execute.mock.calls[0]
    expect(command.endsWith(binary)).toBe(true)
    expect(args.some((arg) => arg.endsWith(script))).toBe(true)
    expect(options.timeout).toBe(45000)
    expect([command, ...args].join(' ')).not.toMatch(/xdotool|\btimeout\b/)
    expect(args).toHaveLength(6)
    expect(args.slice(0, 5)).toEqual(['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File'])
    expect(options.env.MUNIMENT_FOLDER_PATH).toBe(home)
    expect(options.env.MUNIMENT_FOLDER_WAIT_SECONDS).toBe('30')
    expect(args).not.toContain(home)
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
    await expect(chooseFolder(home, 30, title, raw, execute, platform, execute)).rejects.toMatchObject({
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
    await expect(chooseFolder('/tmp/home', 30, title, raw, execute, 'darwin', execute)).rejects.toMatchObject({
      message: `Home picker failed. ${folderDialogDescription(title, 'darwin')}. ${cause.message}`, cause,
    })
  })

  it.each([0, -1, 121, 1.5, NaN, Infinity, '30', undefined])('The picker rejects an invalid wait (%s) before a spawn.', async (wait) => {
    const execute = vi.fn()
    await expect(chooseFolder('/tmp/home', wait, title, await rawDirectory(), execute, 'darwin')).rejects.toThrow('integer from 1 to 120')
    expect(execute).not.toHaveBeenCalled()
  })

  it.each([1, 120])('The picker accepts the wait boundary (%s).', async (wait) => {
    const execute = vi.fn()
    const driveMacos = vi.fn().mockResolvedValue(undefined)
    await chooseFolder('/tmp/home', wait, title, await rawDirectory(), execute, 'darwin', driveMacos)
    expect(driveMacos).toHaveBeenCalledExactlyOnceWith(wait)
    expect(execute).not.toHaveBeenCalled()
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
    const macos = await readFile(new URL('../src-tauri/src/e2e_folder_dialog.rs', import.meta.url), 'utf8')
    expect(macos).toContain('let windows = app.windows()')
    expect(macos).toContain('windows.objectAtIndex(index).downcast::<NSOpenPanel>().ok()')
    expect(macos).toContain('panel.isVisible()')
    expect(macos).toContain('panels.len() > 1')
    const drive = await readFile(new URL('../src-tauri/src/e2e_folder_dialog/drive.rs', import.meta.url), 'utf8')
    expect(drive).toContain('The NSOpenPanel lost keyboard focus.')
    expect(drive).toContain('The NSOpenPanel did not select the isolated Home.')
    expect(drive).toContain('The Home path changed during the picker drive.')
    expect(drive).toContain('The NSOpenPanel did not move to the isolated Home after Return.')
    expect(drive).toContain('now >= self.deadline')
    expect(drive).toContain('The panel directory is')
    expect(macos).toContain('let urls = self.URLs()')
    const windows = await readFile(new URL('./e2e/support/folder-dialog-windows.ps1', import.meta.url), 'utf8')
    expect(windows).toContain("Get-Process -Name 'muniment-desktop'")
    expect(windows).toContain("$_.ProcessId -in $processIds -and $_.Class -eq '#32770'")
    const search = windows.slice(windows.indexOf('function Find-FolderDialog'), windows.indexOf('\ntry {'))
    expect(search).not.toMatch(/IsOffscreen|\.Visible/)
    expect(search).toContain('[MunimentFolderPicker.Desktop]::Windows()')
    expect(windows).toContain('if ($matches.Count -gt 1) { throw')
    expect(windows).toContain('if ([DateTime]::UtcNow -ge $deadline) { throw')
    expect(windows).toContain("AutomationIdProperty, '1152'")
    expect(windows).toContain('[System.Windows.Automation.ControlType]::Edit')
    expect(windows).toContain('$value.SetValue($homePath)')
    expect(windows).toContain('$_.Handle -eq $handle -and $_.Visible')
    expect(windows).not.toMatch(/SendKeys|SendWait|Invoke-Expression/)
  })

  it('The macOS picker uses trusted CGEvent keys through the app IPC without requesting a grant.', async () => {
    const invoke = vi.fn().mockResolvedValue(true)
    const execute = vi.fn()
    vi.stubGlobal('window', { __TAURI__: { core: { invoke } } })
    const browserExecute = vi.fn((callback) => callback())
    vi.stubGlobal('browser', { execute: browserExecute })
    await chooseFolder('/tmp/home', 30, title, await rawDirectory(), execute, 'darwin')
    expect(execute).not.toHaveBeenCalled()
    expect(browserExecute).toHaveBeenCalledTimes(1)
    expect(invoke).toHaveBeenCalledExactlyOnceWith('e2e_drive_folder_dialog')

    const native = await readFile(new URL('../src-tauri/src/e2e_folder_dialog.rs', import.meta.url), 'utf8')
    expect(native).toContain('event.post(CGEventTapLocation::HID)')
    expect(native).toContain('AXIsProcessTrusted()')
    expect(native).toContain('CGEvent::new_keyboard_event')
    expect(native).toContain('for character in home.chars()')
    expect(native).toContain('character.encode_utf16(&mut units)')
    expect(native).toContain('app.isActive()')
    expect(native).toMatch(/window\s*\.sheetParent\(\)/)
    expect(native).toContain('run_on_main_thread')
    expect(native).toContain('MUNIMENT_E2E_ONBOARDING_ONLY')
    expect(native).not.toMatch(/AXUIElement|AXIsProcessTrustedWithOptions|osascript|Command::new|sendEvent|keyEventWithType|setDirectoryURL|\b(?:self|panel)\.ok\(/)
    const drive = await readFile(new URL('../src-tauri/src/e2e_folder_dialog/drive.rs', import.meta.url), 'utf8')
    expect(drive.indexOf('if !self.panel.is_trusted()')).toBeLessThan(drive.indexOf('.send_step(self.step, home)'))
    expect(drive).toContain('AXIsProcessTrusted() returned false')
    const main = await readFile(new URL('../src-tauri/src/main.rs', import.meta.url), 'utf8')
    expect(main).toMatch(/#\[cfg\(all\(target_os = "macos", feature = "e2e-webdriver"\)\)\]\s*mod e2e_folder_dialog/)
    expect(main).toMatch(/#\[cfg\(all\(target_os = "macos", feature = "e2e-webdriver"\)\)\]\s*e2e_folder_dialog::e2e_drive_folder_dialog/)
    expect(main).toMatch(/#\[cfg\(all\(target_os = "macos", feature = "e2e-webdriver"\)\)\]\s*e2e_folder_dialog::e2e_folder_dialog_snapshot/)
    const snapshot = native.split('pub(crate) async fn e2e_folder_dialog_snapshot')[1].split('pub(crate) async fn e2e_drive_folder_dialog')[0]
    expect(snapshot).toContain('MUNIMENT_E2E_ONBOARDING_ONLY')
    expect(snapshot).toContain('run_on_main_thread')
    expect(snapshot).toContain('windows: (0..windows.len())')
    expect(snapshot).toContain('class: window.class().name().to_string_lossy().into_owned()')
    expect(snapshot).toContain('title: window.title().to_string()')
    expect(snapshot).toContain('visible: window.isVisible()')
    expect(snapshot).not.toMatch(/\.filter|sendEvent|key\(/)
    const ci = await readFile(new URL('../.github/workflows/ci.yml', import.meta.url), 'utf8')
    expect(ci).toContain('cargo check --manifest-path src-tauri/Cargo.toml --package muniment-desktop --locked --features e2e-webdriver --target aarch64-apple-darwin')
  })

  it('The macOS E2E runner runs the production drive against a real folder panel.', async () => {
    const nativeTest = await readFile(new URL('../src-tauri/tests/folder_dialog_macos.rs', import.meta.url), 'utf8')
    expect(nativeTest).toContain('#[path = "../src"]')
    expect(nativeTest).toContain('pub mod e2e_folder_dialog')
    expect(nativeTest).toContain('NSOpenPanel::openPanel(mtm)')
    expect(nativeTest).toContain('panel.setCanChooseDirectories(true)')
    expect(nativeTest).toContain('panel.beginWithCompletionHandler(&handler)')
    expect(nativeTest).toContain('e2e_folder_dialog::poll(home.clone())')
    expect(nativeTest).toContain('Some((NSModalResponseOK, vec![home.clone()]))')
    expect(nativeTest).toContain('SKIP folder-dialog-macos: CGSessionCopyCurrentDictionary found no window server session.')
    expect(nativeTest).toContain('SKIP folder-dialog-macos: AXIsProcessTrusted() returned false.')
    const runner = await readFile(new URL('./e2e/runner/macos-wdio.sh', import.meta.url), 'utf8')
    expect(runner).toContain('run_step folder-dialog-macos log_command "$raw/folder-dialog-macos.log" cargo test')
    expect(runner).toContain('--test folder_dialog_drive --test folder-dialog-macos || exit')
    expect(runner.indexOf('run_step folder-dialog-macos')).toBeGreaterThan(runner.indexOf('run_step verify-webdriver-signature'))
  })

  it('The AppKit drive waits for the native panel to close.', async () => {
    vi.useFakeTimers()
    const poll = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(false).mockResolvedValue(true)
    const result = driveMacosFolder(1, poll)
    await vi.runAllTimersAsync()
    await result
    expect(poll).toHaveBeenCalledTimes(3)
    expect(vi.getTimerCount()).toBe(0)
  })

  it.each(['absent panel', 'blocked IPC'])('The AppKit drive bounds a wait for %s.', async (state) => {
    vi.useFakeTimers()
    const poll = vi.fn(() => state === 'blocked IPC' ? new Promise(() => {}) : Promise.resolve(false))
    const result = expect(driveMacosFolder(1, poll)).rejects.toThrow('NSOpenPanel timed out')
    await vi.advanceTimersByTimeAsync(1000)
    await result
    const calls = poll.mock.calls.length
    await vi.runAllTimersAsync()
    expect(poll).toHaveBeenCalledTimes(calls)
    expect(vi.getTimerCount()).toBe(0)
  })

  it.each([
    { status: 'not-started' },
    { status: 'pending' },
    { status: 'resolved', value: null },
    { status: 'resolved', value: '/tmp/home' },
    { status: 'rejected', error: 'The dialog command failed.' },
  ])('An absent panel records every window and the open outcome ($status).', async (open) => {
    const raw = await rawDirectory()
    vi.useFakeTimers()
    const native = {
      windows: [
        { class: 'TaoWindow', title: 'muniment', visible: true },
        { class: 'NSPanel', title: '', visible: false },
        { class: 'NSOpenPanel', title: 'Choose "Home"', visible: false },
      ],
      step: null,
    }
    const invoke = vi.fn((command) => Promise.resolve(command === 'e2e_drive_folder_dialog' ? false : native))
    vi.stubGlobal('window', { __TAURI__: { core: { invoke } } })
    vi.stubGlobal('document', { querySelector: () => ({ getAttribute: () => JSON.stringify(open) }) })
    vi.stubGlobal('browser', { execute: (callback) => callback() })
    const failure = chooseFolder('/tmp/home', 1, title, raw, vi.fn(), 'darwin').catch((error) => error)
    await vi.advanceTimersByTimeAsync(1000)
    const error = await failure
    expect(error.message).toContain('NSOpenPanel timed out')
    expect(error.message).toContain(JSON.stringify({ native, open }))
    expect(await readFile(path.join(raw, 'folder-picker-failure.log'), 'utf8')).toContain(error.message)
    expect(invoke).toHaveBeenLastCalledWith('e2e_folder_dialog_snapshot')
    const calls = invoke.mock.calls.length
    await vi.runAllTimersAsync()
    expect(invoke).toHaveBeenCalledTimes(calls)
    expect(vi.getTimerCount()).toBe(0)
  })

  it.each(['rejected', 'blocked'])('The open outcome survives a %s native snapshot.', async (state) => {
    vi.useFakeTimers()
    vi.stubGlobal('window', { __TAURI__: { core: { invoke: () => state === 'blocked'
      ? new Promise(() => {}) : Promise.reject('The AppKit thread is unavailable.') } } })
    vi.stubGlobal('document', { querySelector: () => ({ getAttribute: () => '{"status":"pending"}' }) })
    vi.stubGlobal('browser', { execute: (callback) => callback() })
    const failure = driveMacosFolder(1, () => false).catch((error) => error)
    await vi.advanceTimersByTimeAsync(3000)
    const error = await failure
    expect(error.message).toContain('NSOpenPanel timed out')
    expect(error.message).toContain('"open":{"status":"pending"}')
    expect(error.message).toContain(state === 'blocked' ? 'The AppKit snapshot timed out.' : 'The AppKit thread is unavailable.')
    expect(vi.getTimerCount()).toBe(0)
  })

  it('The AppKit drive bounds a blocked diagnostic request.', async () => {
    vi.useFakeTimers()
    const failure = driveMacosFolder(1, () => false, () => new Promise(() => {})).catch((error) => error)
    await vi.advanceTimersByTimeAsync(4000)
    expect((await failure).message).toContain('NSOpenPanel timed out during the AppKit picker drive. Home picker diagnostics: The Home picker diagnostics timed out.')
    expect(vi.getTimerCount()).toBe(0)
  })

  it('An empty window list stays explicit in the failure.', async () => {
    const cause = new Error('The NSOpenPanel lost keyboard focus.')
    const diagnostics = { native: { windows: [], step: 2 }, open: { status: 'unavailable' } }
    await expect(driveMacosFolder(1, () => Promise.reject(cause), () => diagnostics)).rejects.toMatchObject({
      cause, message: `${cause.message} Home picker diagnostics: ${JSON.stringify(diagnostics)}`,
    })
  })

  it('The AppKit drive keeps a string IPC error when diagnostics fail.', async () => {
    const cause = 'The Home picker needs the AppKit thread.'
    await expect(driveMacosFolder(1, () => Promise.reject(cause), () => Promise.reject('The webview closed.'))).rejects.toMatchObject({
      cause, message: `${cause} Home picker diagnostics: The webview closed.`,
    })
  })

  it.each([undefined, null, 'true', 1])('The AppKit drive rejects an invalid result (%s).', async (result) => {
    await expect(driveMacosFolder(1, vi.fn().mockResolvedValue(result))).rejects.toThrow('invalid state')
  })

  it('The AppKit drive reports native errors without a retry.', async () => {
    const poll = vi.fn().mockRejectedValue(new Error('The NSOpenPanel lost keyboard focus.'))
    await expect(driveMacosFolder(1, poll)).rejects.toThrow('lost keyboard focus')
    expect(poll).toHaveBeenCalledTimes(1)
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
    expect(spec).toContain('await withWindowsPickerDiagnostics(async () => {')
    expect(spec).toContain("document.querySelector('[data-home-picker]')?.getAttribute('data-home-picker')")
    expect(spec).toContain('await homePathMatches(location, home)')
    expect(spec).toContain("expect(await location.getProperty('textContent')).toBe(home)")
    expect(spec).toContain('${folderDialogDescription(FOLDER_DIALOG_TITLE)}')
    expect(spec).not.toMatch(/mock|plugin:dialog\|open/)
  })
})
