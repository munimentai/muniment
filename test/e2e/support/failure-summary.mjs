import fs from 'node:fs'
import { redactText } from './redact-text.mjs'

const [stdoutFile, stderrFile, command, exitCode] = process.argv.slice(2)
const input = stdoutFile
  ? { stdout: fs.readFileSync(stdoutFile, 'utf8'), stderr: fs.readFileSync(stderrFile, 'utf8'), label: `${command} failed`, exitCode }
  : JSON.parse(fs.readFileSync(0, 'utf8'))

function bounded(text, budget) {
  const characters = Array.from(text)
  if (characters.length <= budget) return text
  const head = Math.floor(budget / 2)
  return characters.slice(0, head).join('') + ' ... ' + characters.slice(-(budget - head - 5)).join('')
}

const streams = [input.stdout, input.stderr].map((text) => {
  // Redact complete streams before a cutoff can split a secret or token.
  const safe = redactText(text).trim()
  // Node prints source excerpts before the error and stack frames after it.
  const diagnostics = safe.split(/\r?\n/).filter((line) => /^\s*(?:[\w.]*Error(?:\s*\[[^\]]+\])?|error|fatal)\s*:/i.test(line))
  return diagnostics.length ? diagnostics.join('\n') : safe
}).filter(Boolean)
const budget = streams.length > 1 ? 400 : 850
const detail = streams.map((text) => bounded(text, budget)).join('\n')
const label = Array.from(redactText(input.label)).slice(0, 100).join('')
process.stdout.write(`${label} (exit code ${input.exitCode})${detail ? `: ${detail}` : ''}`)
