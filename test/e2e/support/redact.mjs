import fs from 'node:fs'
import path from 'node:path'

const [source, destination, failureReport] = process.argv.slice(2)
if (!source || !destination) throw new Error('usage: redact.mjs SOURCE DESTINATION')

const secrets = ['MUNIMENT_E2E_USERNAME', 'MUNIMENT_E2E_PASSWORD', 'GH_TOKEN']
  .map((name) => process.env[name]).filter(Boolean)
const tokenPatterns = [
  ['credential-header', /\b(?:authorization|cookie|set-cookie)\s*[:=][^\r\n]+/gi],
  ['bearer-token', /\bBearer\s+[A-Za-z0-9._~+\/-]+=*/gi],
  ['oauth-token', /\b(?:access|refresh|id)_token\s*[:=]\s*[^\s,}\]]+/gi],
  ['github-token', /\bgh[opsu]_[A-Za-z0-9]{20,}\b/g],
  ['jwt', /\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b/g],
]

const safeScreenshots = new Set(['01-signed-out.png', '02-authenticated.png'])
const isSafeScreenshot = (name) => safeScreenshots.has(name) || /^screenshot-[A-Za-z0-9._-]+\.png$/.test(name)
function fail(file, category) {
  const detail = `file: ${JSON.stringify(file)}\ncategory: ${category}\n`
  if (failureReport) fs.writeFileSync(failureReport, detail, { mode: 0o600 })
  throw new Error(`artifact redaction failed for ${JSON.stringify(file)} (${category})`)
}
function inspectScreenshot(input) {
  const data = fs.readFileSync(input)
  const name = path.basename(input)
  if (data.length < 33 || !data.subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10]))) fail(name, 'screenshot-format')
  // Boundary captures are element crops. Remove ancillary metadata and reject
  // any injected value embedded in the binary.
  for (const secret of secrets) if (data.includes(Buffer.from(secret))) fail(name, 'verbatim-injected-secret')
  let offset = 8
  const allowed = new Set(['IHDR', 'PLTE', 'IDAT', 'IEND', 'tRNS'])
  const chunks = [data.subarray(0, 8)]
  while (offset + 12 <= data.length) {
    const length = data.readUInt32BE(offset); const type = data.toString('ascii', offset + 4, offset + 8)
    if (offset + 12 + length > data.length) fail(name, 'screenshot-metadata')
    if (allowed.has(type)) chunks.push(data.subarray(offset, offset + 12 + length))
    else if ((data[offset + 4] & 0x20) === 0) fail(name, 'screenshot-format')
    offset += 12 + length
    if (type === 'IEND') {
      if (offset !== data.length) fail(name, 'screenshot-trailing-data')
      return Buffer.concat(chunks)
    }
  }
  fail(name, 'screenshot-truncated')
}

// Verbatim injected secrets show that the capture boundary failed. Credential
// forms in page sources and logs are expected near sign-in flows and get
// redacted below.
for (const entry of fs.readdirSync(source, { withFileTypes: true })) {
  if (!entry.isFile()) continue
  const input = path.join(source, entry.name)
  if (/\.png$/i.test(entry.name)) {
    if (!isSafeScreenshot(entry.name)) fail(entry.name, 'unapproved-screenshot')
    inspectScreenshot(input)
  } else {
    const text = fs.readFileSync(input, 'utf8')
    if (secrets.some((secret) => text.includes(secret))) fail(entry.name, 'verbatim-injected-secret')
  }
}

fs.mkdirSync(destination, { recursive: true, mode: 0o700 })
for (const entry of fs.readdirSync(source, { withFileTypes: true })) {
  if (!entry.isFile()) continue
  const input = path.join(source, entry.name)
  const output = path.join(destination, entry.name.replace(/[^A-Za-z0-9._-]/g, '_'))
  if (/\.png$/i.test(entry.name)) {
    fs.writeFileSync(output, inspectScreenshot(input), { mode: 0o600 })
    continue
  }
  let text = fs.readFileSync(input, 'utf8')
  for (const secret of secrets) text = text.split(secret).join('[REDACTED]')
  for (const [category, pattern] of tokenPatterns) text = text.replace(pattern, `[REDACTED:${category}]`)
  fs.writeFileSync(output, text, { mode: 0o600 })
}

// Fail closed: artifact emission is blocked if an injected value or a known
// credential/header form survived redaction. Never print the matching value.
for (const file of fs.readdirSync(destination)) {
  if (/\.png$/i.test(file)) continue
  const text = fs.readFileSync(path.join(destination, file), 'utf8')
  if (secrets.some((secret) => text.includes(secret))) {
    fs.rmSync(destination, { recursive: true, force: true })
    fail(file, 'verbatim-injected-secret')
  }
  for (const [category, pattern] of tokenPatterns) {
    pattern.lastIndex = 0
    if (pattern.test(text)) {
      fs.rmSync(destination, { recursive: true, force: true })
      fail(file, category)
    }
  }
}
