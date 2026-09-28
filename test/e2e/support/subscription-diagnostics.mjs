import fs from 'node:fs'
import path from 'node:path'
import { redactText } from './redact-text.mjs'

// Register secrets before parsing leases so parser errors cannot expose their values.
export function subscriptionRedactor({ leases, values = [] } = {}) {
  const secrets = [...values, leases]
  let invalidLeases = false
  try {
    const visit = value => {
      if (typeof value === 'string') secrets.push(value)
      else if (value && typeof value === 'object') Object.values(value).forEach(visit)
    }
    const parsed = JSON.parse(leases)
    secrets.push(JSON.stringify(parsed))
    visit(parsed)
  } catch { invalidLeases = Boolean(leases) }
  for (const [name, value] of Object.entries(process.env)) {
    if (/TOKEN|SECRET|PASSWORD|LEASES|SSH_KEY/.test(name)) secrets.push(value)
  }
  const forms = [...new Set(secrets.filter(value => typeof value === 'string' && value).flatMap(value => {
    const forms = [value, JSON.stringify(value).slice(1, -1), Buffer.from(value).toString('base64')]
    try { forms.push(encodeURIComponent(value)) } catch { /* Invalid Unicode has no URI form. */ }
    return forms
  }))].sort((a, b) => b.length - a.length)
  return value => {
    if (invalidLeases) return 'The lease JSON is invalid. Diagnostic details cannot appear in the log.\n'
    let text = String(value ?? '')
    for (const secret of forms) text = text.split(secret).join('[REDACTED]')
    text = redactText(text)
      .replace(/\b(?:authorization|cookie|set-cookie)["']?\s*[:=]\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\r\n]+)/gi, '[REDACTED:header]')
      .replace(/\b(?:access|refresh|id)[_-]?token["']?\s*[:=]\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s,}\]]+)/gi, '[REDACTED:token]')
      .replace(/\b(?:chatgpt[-_])?account[-_]?id["']?\s*[:=]\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s,}\]]+)/gi, '[REDACTED:account]')
    // Short or overlapping injected values can also occur inside replacement labels.
    if (forms.some(secret => text.includes(secret))) return 'The diagnostic contains a secret and cannot appear in the log.\n'
    return text
  }
}

export const diagnosticTail = (text, redact, limit = 16_384) => redact(text).slice(-limit)

export function reportSubscriptionSummary(platform, status, reason, diagnostics, redact, emit = console.error) {
  const safeReason = diagnosticTail(typeof reason === 'string' && reason ? reason : 'none', redact, 2048)
  emit(`platform=${platform} status=${status}\nreason=${JSON.stringify(safeReason)}`)
  if (diagnostics) {
    const tail = diagnosticTail(transcriptText(redact(diagnostics)), redact)
    emit(`Diagnostic tail:\n${tail.split('\n').map(line => `  ${line}`).join('\n')}`)
  }
}

export function reportSubscriptionFailure(output, platform, status, redact) {
  let evidence, proof
  try {
    evidence = JSON.parse(fs.readFileSync(path.join(output, `${platform}-subscription.json`), 'utf8'))
    proof = JSON.parse(fs.readFileSync(path.join(output, 'release-acceptance.json'), 'utf8'))
  } catch { /* Report missing or malformed evidence without parser excerpts. */ }
  const cases = Array.isArray(proof?.cases) ? proof.cases.filter(item => item?.platform === platform) : []
  if (status === 0 && evidence?.status === 'passed' && cases.length && cases.every(item => item?.status === 'passed')) return
  const reason = evidence?.reason || cases.find(item => item?.status !== 'passed')?.reason ||
    'The native check did not produce passing evidence.'
  let diagnostics = 'No diagnostic log exists.'
  try { diagnostics = fs.readFileSync(path.join(output, `${platform}-subscription.log`), 'utf8') }
  catch { /* Keep the summary when the log is missing or unreadable. */ }
  reportSubscriptionSummary(platform, 'blocked', reason, diagnostics, redact)
}

const payloadMarker = 'payload-difference='

export function payloadDifferenceDetail(difference, redact = subscriptionRedactor()) {
  const paths = {}
  for (const name of ['missing', 'extra', 'differing']) {
    if (!Array.isArray(difference?.[name])) return ''
    // Bound diagnostics without limiting the payload comparison.
    paths[name] = difference[name].slice(0, 50).map(value => {
      // Reject absolute paths and control characters before any console or Markdown output.
      if (typeof value !== 'string' || !value || /^[\\/]|^[a-z]:/i.test(value) ||
          /[\x00-\x1f\x7f]/.test(value) || value.split(/[\\/]/).some(part => !part || part === '.' || part === '..')) {
        return '[invalid path]'
      }
      const safe = redact(value)
      // Do not cut through a credential or user identifier at the length limit.
      return safe.length > 512 ? '[path exceeds limit]' : safe
    })
  }
  return payloadMarker + JSON.stringify(paths)
}

export function reportPayloadDifferences(text, redact, { emit = console.error, summary = process.env.GITHUB_STEP_SUMMARY } = {}) {
  const reports = new Set()
  for (const line of text.split('\n')) {
    if (!line.startsWith(payloadMarker)) continue
    try {
      const detail = payloadDifferenceDetail(JSON.parse(line.slice(payloadMarker.length)), redact)
      if (detail) reports.add(detail)
    } catch { /* An incomplete diagnostic cannot supply path lists. */ }
    if (reports.size === 3) break
  }
  for (const detail of reports) {
    emit(detail)
    if (summary) {
      const escaped = detail.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;')
      fs.appendFileSync(summary, `\n<pre>${escaped}</pre>\n`)
    }
  }
}

export function nativeFailure(command, result) {
  // A timeout or buffer limit can split a secret at the last line.
  const output = value => {
    const text = String(value ?? '')
    return result.error ? text.slice(0, text.lastIndexOf('\n') + 1) : text
  }
  return new Error(`${command}: status=${result.status ?? 'none'} signal=${result.signal ?? 'none'} error=${result.error?.message ?? 'none'}\nstdout:\n${output(result.stdout)}\nstderr:\n${output(result.stderr)}`)
}

export function processStatus(child) {
  if (!child) return 'not started'
  if (child.diagnosticError) return `spawn failed: ${child.diagnosticError}`
  if (!child.pid) return 'spawn failed'
  return `pid=${child.pid} exit=${child.exitCode ?? 'none'} signal=${child.signalCode ?? 'none'}`
}

export function probeProgress(env, step, phase) {
  try {
    const checkpoint = path.join(env.MUNIMENT_STATE_DIR, 'subscription-probe.json')
    const current = JSON.parse(fs.readFileSync(checkpoint, 'utf8')).phase
    if (['chat', 'features', 'restart', 'update', 'update-restart'].includes(current)) phase = current
  } catch { /* Keep the last phase when the checkpoint is missing or incomplete. */ }
  return `step=${step}\nphase=${phase}`
}

export function linuxRuntimeStatus(env) {
  if (!env) return 'not started'
  try {
    const file = path.join(env.XDG_RUNTIME_DIR, 'muniment/desktop-runtime.json')
    if (!fs.existsSync(file)) return 'No runtime identity exists. exit=unavailable'
    if (!fs.lstatSync(file).isFile() || fs.statSync(file).size > 4096) return 'The runtime identity is invalid. exit=unavailable'
    const identity = JSON.parse(fs.readFileSync(file, 'utf8'))
    if (!Number.isSafeInteger(identity.pid) || identity.pid <= 1 || identity.pid > 2147483647 ||
        !Number.isSafeInteger(identity.started) || identity.started <= 0) return 'The runtime identity is invalid. exit=unavailable'
    // A matching receipt survives reaping and PID reuse. Live children have empty receipts.
    const receipt = path.join(path.dirname(file), `desktop-runtime-${identity.pid}-${identity.started}.exit`)
    try {
      const info = fs.lstatSync(receipt)
      if (info.isFile() && info.size <= 16) {
        const text = fs.readFileSync(receipt, 'utf8')
        if (/^\d+\n$/.test(text)) {
          const status = Number(text)
          const signal = status & 127
          const exited = (status & 255) === 0
          const signaled = status <= 255 && signal > 0 && signal <= 64
          if (Number.isSafeInteger(status) && status >= 0 && status <= 65535 && (exited || signaled)) {
            return `pid=${identity.pid} exit=${signal ? 'none' : status >> 8} signal=${signal || 'none'}`
          }
        }
      }
    } catch { /* Older packages have no exit receipt. */ }
    let stat
    try { stat = fs.readFileSync(`/proc/${identity.pid}/stat`, 'utf8') }
    catch { return `pid=${identity.pid} state=absent exit=unavailable` }
    const fields = stat.slice(stat.lastIndexOf(')') + 2).trim().split(/\s+/)
    if (Number(fields[19]) !== identity.started) return 'The runtime identity is stale. exit=unavailable'
    const status = Number(fields[49])
    if (fields[0] === 'Z' && Number.isSafeInteger(status) && status >= 0) {
      return `pid=${identity.pid} exit=${status & 127 ? 'none' : status >> 8} signal=${status & 127 || 'none'}`
    }
    return `pid=${identity.pid} state=${fields[0]} exit=none signal=none`
  } catch { return 'The runtime identity is unreadable. exit=unavailable' }
}

export function readDiagnosticLog(file, root, redact) {
  try {
    if (!fs.existsSync(file)) return 'No log exists.\n'
    const relative = path.relative(fs.realpathSync(root), fs.realpathSync(file))
    if (relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) return 'The log leaves the disposable profile.\n'
    const stat = fs.lstatSync(file)
    if (!stat.isFile()) return 'The log is not a regular file.\n'
    const fd = fs.openSync(file, 'r')
    let text
    try {
      const opened = fs.fstatSync(fd)
      if (opened.dev !== stat.dev || opened.ino !== stat.ino) return 'The log changed during the read.\n'
      const header = Buffer.alloc(2)
      fs.readSync(fd, header, 0, 2, 0)
      const utf16 = header[0] === 0xff && header[1] === 0xfe
      let offset = Math.max(0, opened.size - 65_536)
      if (utf16) offset -= offset % 2
      const bytes = Buffer.alloc(opened.size - offset)
      const length = fs.readSync(fd, bytes, 0, bytes.length, offset)
      text = bytes.subarray(0, length).toString(utf16 ? 'utf16le' : 'utf8')
      // Drop the first partial line before redaction so a cutoff cannot expose half a secret.
      if (offset > 0) text = text.includes('\n') ? text.slice(text.indexOf('\n') + 1) : ''
    } finally { fs.closeSync(fd) }
    // A live process can leave half a secret at EOF. Keep only complete lines.
    const complete = text.slice(0, text.lastIndexOf('\n') + 1)
    return diagnosticTail(complete, redact) + (complete.length < text.length ? '\nThe redactor omits the final incomplete line.\n' : '\n')
  } catch (error) { return `${redact(error.message)}\n` }
}

// Read only named logs from the disposable profile. Never walk credentials or conversations.
export function profileLogs(env, redact) {
  if (!env) return 'The disposable profile has not started.\n'
  const files = [
    ['app', path.join(env.TMPDIR, 'subscription-app.log')],
    ['runtime', path.join(env.TMPDIR, 'subscription-runtime.log')],
    ['msi-admin', path.join(env.TMPDIR, 'subscription-msi-admin.log')],
    ['cef', path.join(env.MUNIMENT_STATE_DIR, 'browser/cef.log')],
    ['keychain', path.join(env.MUNIMENT_STATE_DIR, 'browser/keychain-audit.log')],
    ['runtime-service', path.join(env.HOME, 'Library/Logs/Muniment/runtime-service.log')],
    ['runtime-native', path.join(env.LOCALAPPDATA, 'ai.muniment.desktop/logs/runtime.log')],
  ]
  return files.map(([name, file]) => `${name} tail:\n${readDiagnosticLog(file, path.dirname(env.TMPDIR), redact)}`).join('')
}

// Artifact envelopes contain encoded archives, not readable native diagnostics.
export function transcriptText(text) {
  let envelope = false
  return String(text).split('\n').filter(line => {
    if (/DESKTOP-CI.*(?:BEGIN|END)/.test(line)) {
      envelope = /BEGIN/.test(line)
      return false
    }
    return !envelope && !/^[A-Za-z0-9+/=]{120,}\r?$/.test(line)
  }).join('\n')
}
