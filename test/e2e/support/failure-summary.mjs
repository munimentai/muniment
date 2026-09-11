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

const streams = [input.stderr, input.stdout].map((text) => {
  // Redact complete streams before a cutoff can split a secret or token.
  const safe = redactText(text).replace(/\x1b\[[0-9;]*m/g, '').trim()
  // Keep shell, Rust, npm, and Node errors ahead of progress and source excerpts.
  const diagnostics = safe.split(/\r?\n/).filter((line) => /\bnot recognized\b|^\s*(?:npm\s+(?:ERR!|error)(?=\s|$)|error\[E\d+\]|(?:[\w.]*Error(?:\s*\[[^\]]+\])?|error|fatal)\s*:)/i.test(line))
  return { text: diagnostics.length ? diagnostics.join('\n') : safe, diagnostic: diagnostics.length > 0 }
}).filter(({ text }) => text).sort((a, b) => Number(b.diagnostic) - Number(a.diagnostic))
const budget = streams.length > 1 ? 400 : 850
const detail = streams.map(({ text }) => bounded(text, budget)).join('\n')
const label = Array.from(redactText(input.label)).slice(0, 100).join('')
process.stdout.write(`${label} (exit code ${input.exitCode})${detail ? `: ${detail}` : ''}`)
