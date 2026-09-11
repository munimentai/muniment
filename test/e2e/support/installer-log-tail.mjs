import fs from 'node:fs'
import { redactText } from './redact-text.mjs'

// Redact the full log before a line cutoff can split a secret.
const text = redactText(fs.readFileSync(process.argv[2], 'utf8')).replace(/\r?\n$/, '')
process.stdout.write(`dci: installer.log last 40 lines\n${text.split(/\r?\n/).slice(-40).join('\n')}\n`)
