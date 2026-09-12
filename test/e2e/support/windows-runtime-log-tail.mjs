import fs from 'node:fs'
import path from 'node:path'
import { redactText } from './redact-text.mjs'

// Read only the named diagnostic file. Never collect the user's runtime state.
const roots = JSON.parse(fs.readFileSync(0, 'utf8'))
for (const root of [...new Set(roots.filter(Boolean))]) {
  const file = path.join(root, 'muniment', 'logs', 'runtime.log')
  process.stdout.write(`dci: Windows runtime.log ${redactText(file)}\n`)
  try {
    const descriptor = fs.openSync(file, 'r')
    let text
    try {
      const size = fs.fstatSync(descriptor).size
      if (size > 1024 * 1024) throw new Error('The runtime.log exceeds the 1 MiB diagnostic limit.')
      const buffer = Buffer.alloc(size)
      const count = fs.readSync(descriptor, buffer, 0, size, 0)
      text = buffer.subarray(0, count).toString('utf8')
    } finally {
      fs.closeSync(descriptor)
    }
    // Redact before the line cutoff, including secrets that span lines.
    const safe = redactText(text).replace(/\r?\n$/, '')
    process.stdout.write(safe ? `${safe.split(/\r?\n/).slice(-60).join('\n')}\n` : 'The runtime.log is empty.\n')
  } catch (error) {
    process.stdout.write(error.code === 'ENOENT'
      ? 'No runtime.log exists at this path.\n'
      : `Could not read runtime.log: ${redactText(error.message)}\n`)
  }
}
