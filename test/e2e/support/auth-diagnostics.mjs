import path from 'node:path'
import { open, readdir } from 'node:fs/promises'

// Quote only the closed diagnostic grammar, never request data or other log text.
const unavailableLine = 'desktop native-auth response: runtime unavailable'
const authLine = /(?:muniment-runtime|muniment-desktop): native-auth (?:start method=(?:POST|GET|RPC|COMMAND) path=(?:\/v1\/auth\/native\/(?:devices|authorize|token|session|revoke)|session\.sign_in|auth_sign_in)|end method=(?:POST|GET|RPC|COMMAND) path=(?:\/v1\/auth\/native\/(?:devices|authorize|token|session|revoke)|session\.sign_in|auth_sign_in) (?:status=(?:[1-5][0-9]{2}|ok)|error=[A-Za-z]+) elapsed_ms=[0-9]+)$/

export async function lastAuthLogs(directory) {
  let files
  try {
    files = (await readdir(directory)).filter((name) => name.endsWith('.log')).sort()
  } catch {
    return 'The auth logs are unavailable.'
  }
  const found = []
  for (const name of files) {
    let file
    try {
      file = await open(path.join(directory, name), 'r')
      const { size } = await file.stat()
      const offset = Math.max(0, size - 65536)
      const buffer = Buffer.alloc(Math.min(size, 65536))
      const { bytesRead } = await file.read(buffer, 0, buffer.length, offset)
      const text = buffer.subarray(0, bytesRead).toString('utf8')
      // Ignore a partial first line and an unfinished last line.
      const lines = text.split('\n').slice(offset ? 1 : 0, -1)
        .map((line) => {
          const text = line.replace(/\r$/, '')
          return text.match(authLine)?.[0] ?? (text.endsWith(unavailableLine) ? unavailableLine : undefined)
        })
        .filter(Boolean).slice(-6)
      if (lines.length) found.push(lines.map((line) => `"${line}"`).join('\n'))
    } catch {
      // A log can disappear during process cleanup.
    } finally {
      await file?.close().catch(() => {})
    }
  }
  return found.length ? found.join('\n') : 'The auth logs contain no native-auth lines.'
}

export function withAuthDiagnostics(step, directory) {
  return async function () {
    try {
      return await step.call(this)
    } catch (error) {
      const message = error instanceof Error ? error.message : 'The sign-in spec failed.'
      throw new Error(`${message}\n${await lastAuthLogs(directory)}`, { cause: error })
    }
  }
}
