import path from 'node:path'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { appendFile } from 'node:fs/promises'

const run = promisify(execFile)

export async function writeFolderDialogTimeoutArtifact(title, rawDir, execute = run) {
  const lines = [`searched title: ${title}`, 'visible window titles:']
  try {
    const { stdout } = await execute('xdotool', ['search', '--onlyvisible', '--name', '.*'])
    const windows = stdout.trim().split('\n').filter(Boolean)
    for (const window of windows) {
      try {
        const { stdout: windowTitle } = await execute('xdotool', ['getwindowname', window])
        lines.push(`${window}: ${windowTitle.trim()}`)
      } catch {
        lines.push(`${window}: title unavailable`)
      }
    }
    if (windows.length === 0) lines.push('(none)')
  } catch (error) {
    lines.push(`window search failed: ${error.message}`)
  }
  await appendFile(path.join(rawDir, 'folder-picker-timeout.log'), `${lines.join('\n')}\n`)
}

export async function chooseFolder(home, waitSeconds, title, rawDir, execute = run) {
  let stdout
  try {
    ({ stdout } = await execute('timeout', [
      String(waitSeconds), 'xdotool', 'search', '--sync', '--onlyvisible', '--name', title,
    ]))
  } catch (error) {
    if (error.code === 124) {
      try { await writeFolderDialogTimeoutArtifact(title, rawDir, execute) } catch {}
    }
    throw error
  }
  const window = stdout.trim().split('\n').at(-1)
  await execute('xdotool', ['windowfocus', '--sync', window])
  await execute('xdotool', ['key', '--window', window, '--clearmodifiers', 'ctrl+l'])
  await execute('xdotool', ['type', '--window', window, '--clearmodifiers', '--delay', '1', home])
  await execute('xdotool', ['key', '--window', window, '--clearmodifiers', 'Return'])
  await new Promise((resolve) => setTimeout(resolve, 500))
  try {
    await execute('xdotool', ['getwindowname', window])
  } catch {
    return
  }
  await execute('xdotool', ['key', '--window', window, '--clearmodifiers', 'alt+s'])
}
