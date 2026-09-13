import path from 'node:path'
import { execFile } from 'node:child_process'
import { appendFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
import { chooseFolder as chooseLinuxFolder } from './folder-dialog.mjs'
import { driveMacosFolder } from './folder-dialog-macos.mjs'

const run = promisify(execFile)

export function folderDialogDescription(title, platform = process.platform) {
  const window = {
    linux: title,
    darwin: 'NSOpenPanel (Open or Choose) in ai.muniment.desktop',
    win32: 'shell folder dialog (#32770) in muniment-desktop',
  }[platform] ?? title
  const name = { darwin: 'macos', win32: 'windows' }[platform] ?? platform
  return `platform: ${name}. searched window: ${window}`
}

export async function withWindowsPickerDiagnostics(action, readOutcome, rawDir, platform = process.platform) {
  if (platform !== 'win32') return action()
  let result
  let failure
  let failed = false
  try {
    result = await action()
  } catch (error) {
    failure = error
    failed = true
  }
  const failureMessage = failure instanceof Error ? failure.message : String(failure)
  let outcome
  try {
    outcome = await readOutcome() ?? 'The picker outcome is absent.'
  } catch (error) {
    outcome = `The picker outcome query failed: ${error instanceof Error ? error.message : String(error)}`
  }
  const message = `Home picker open() outcome: ${typeof outcome === 'string' ? outcome : JSON.stringify(outcome)}`
  try {
    await appendFile(path.join(rawDir, failed ? 'folder-picker-failure.log' : 'folder-picker-outcome.log'), `${failed ? `${failureMessage}\n` : ''}${message}\n`)
  } catch {}
  if (failed) throw new Error(`${failureMessage} ${message}`, { cause: failure })
  return result
}

export async function chooseFolder(home, waitSeconds, title, rawDir, execute = run, platform = process.platform, driveMacos = driveMacosFolder) {
  const description = folderDialogDescription(title, platform)
  try {
    if (!['linux', 'darwin', 'win32'].includes(platform)) throw new Error('The folder picker does not support this platform.')
    if (!Number.isSafeInteger(waitSeconds) || waitSeconds < 1 || waitSeconds > 120) {
      throw new Error('The folder picker wait must be an integer from 1 to 120 seconds.')
    }
    const paths = platform === 'win32' ? path.win32 : path.posix
    if (typeof home !== 'string' || !paths.isAbsolute(home) || /[\u0000-\u001f\u007f]/.test(home)) {
      throw new Error('The folder picker needs an absolute Home path without control characters.')
    }
    if (platform === 'linux') {
      await chooseLinuxFolder(home, waitSeconds, title, rawDir, execute)
    } else if (platform === 'darwin') {
      await driveMacos(waitSeconds)
    } else {
      await execute(path.win32.join(process.env.SystemRoot || 'C:\\Windows', 'System32', 'WindowsPowerShell', 'v1.0', 'powershell.exe'), [
        '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File',
        fileURLToPath(new URL('./folder-dialog-windows.ps1', import.meta.url)),
      ], {
        // Reserve time for PowerShell startup and the native window report.
        timeout: (waitSeconds + 15) * 1000,
        maxBuffer: 8 * 1024 * 1024,
        env: { ...process.env, MUNIMENT_FOLDER_PATH: home, MUNIMENT_FOLDER_WAIT_SECONDS: String(waitSeconds) },
      })
    }
  } catch (error) {
    const message = `Home picker failed. ${description}. ${error.message}`
    try {
      await appendFile(path.join(rawDir, 'folder-picker-failure.log'), `${message}\n${error.stdout || ''}\n${error.stderr || ''}\n`)
    } catch {}
    throw new Error(message, { cause: error })
  }
}
