import fs from 'node:fs'
import path from 'node:path'

const [source, destination] = process.argv.slice(2)
if (!source || !destination) throw new Error('usage: redact.mjs SOURCE DESTINATION')

const secrets = ['MUNIMENT_E2E_USERNAME', 'MUNIMENT_E2E_PASSWORD', 'GH_TOKEN']
  .map((name) => process.env[name]).filter(Boolean)
const tokenPatterns = [
  /\b(?:authorization|cookie|set-cookie)\s*[:=][^\r\n]+/gi,
  /\bBearer\s+[A-Za-z0-9._~+\/-]+=*/gi,
  /\b(?:access|refresh|id)_token\s*[:=]\s*[^\s,}\]]+/gi,
  /\bgh[opsu]_[A-Za-z0-9]{20,}\b/g,
  /\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b/g,
]

fs.mkdirSync(destination, { recursive: true, mode: 0o700 })
for (const entry of fs.readdirSync(source, { withFileTypes: true })) {
  if (!entry.isFile()) continue
  const input = path.join(source, entry.name)
  const output = path.join(destination, entry.name.replace(/[^A-Za-z0-9._-]/g, '_'))
  if (/\.png$/i.test(entry.name)) {
    fs.copyFileSync(input, output)
    continue
  }
  let text = fs.readFileSync(input, 'utf8')
  for (const secret of secrets) text = text.split(secret).join('[REDACTED]')
  for (const pattern of tokenPatterns) text = text.replace(pattern, '[REDACTED]')
  fs.writeFileSync(output, text, { mode: 0o600 })
}

// Fail closed: artifact emission is blocked if an injected value or a known
// credential/header form survived redaction. Never print the matching value.
for (const file of fs.readdirSync(destination)) {
  if (/\.png$/i.test(file)) continue
  const text = fs.readFileSync(path.join(destination, file), 'utf8')
  if (secrets.some((secret) => text.includes(secret)) || tokenPatterns.some((pattern) => { pattern.lastIndex = 0; return pattern.test(text) })) {
    fs.rmSync(destination, { recursive: true, force: true })
    throw new Error('artifact redaction scan failed: sensitive-value category detected')
  }
}
