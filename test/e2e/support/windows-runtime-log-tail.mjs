import fs from 'node:fs'
import path from 'node:path'
import { redactText } from './redact-text.mjs'

// Read only named diagnostics. Never collect the user's runtime state.
const roots = JSON.parse(fs.readFileSync(0, 'utf8'))
const files = [...new Set(roots.filter(Boolean))]
  .map((root) => path.join(root, 'ai.muniment.desktop', 'logs', 'runtime.log'))
if (process.argv[2]) files.unshift(path.join(process.argv[2], 'pi-local-mode-stderr.log'))
for (const file of files) {
  const name = path.basename(file)
  process.stdout.write(`dci: Windows ${name} ${redactText(file)}\n`)
  try {
    const descriptor = fs.openSync(file, 'r')
    let text
    try {
      const size = fs.fstatSync(descriptor).size
      if (size > 1024 * 1024) throw new Error(`The ${name} exceeds the 1 MiB diagnostic limit.`)
      const buffer = Buffer.alloc(size)
      const count = fs.readSync(descriptor, buffer, 0, size, 0)
      text = buffer.subarray(0, count).toString('utf8')
    } finally {
      fs.closeSync(descriptor)
    }
    // Keep each run's cause even when later requests add more than 60 lines.
    const safe = redactText(text).replace(/\r?\n$/, '')
    process.stdout.write(safe ? `${safe}\n` : `The ${name} is empty.\n`)
  } catch (error) {
    process.stdout.write(error.code === 'ENOENT'
      ? `No ${name} exists at this path.\n`
      : `Could not read ${name}: ${redactText(error.message)}\n`)
  }
}
