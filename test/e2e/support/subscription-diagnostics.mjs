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
  reportSubscriptionSummary(platform, evidence?.status === 'failed' ? 'failed' : 'blocked', reason, diagnostics, redact)
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

const probeStages = ['composer', 'runtime', 'selection', 'model-save', 'inventory', 'send', 'reply', 'render', 'complete', 'restore', 'features', 'result', 'transport']
const probeErrors = ['none', 'timeout', 'command-timeout', 'command-failed', 'reply-failed', 'context-mismatch', 'auth', 'quota', 'http', 'network', 'stream',
  'update-profile', 'update-plan', 'update-phase', 'update-state', 'update-address', 'update-builder', 'update-check',
  'update-check-network', 'update-check-target-not-found', 'update-check-manifest-parse',
  'update-check-release-not-found', 'update-check-version', 'update-check-address', 'update-check-other',
  'update-download', 'update-unavailable', 'update-not-prepared', 'update-package-digest', 'update-tamper-rejection',
  'update-version-rejection', 'update-active-work-refusal', 'update-checkpoint-encode', 'update-checkpoint-write',
  'update-busy', 'update-install-task', 'update-install', 'update-restart']
const probeCommands = ['attach_listener_status', 'local_mode_provider_inventory', 'chat_current_thread', 'chat_thread_open',
  'chat_answer_permission', 'subscription_probe_progress', 'subscription_probe_observed', 'subscription_probe_update',
  'workspace_read_text', 'workspace_save_text', 'workspace_folders', 'workspace_file_action',
  'model_router_settings', 'model_router_update_account', 'model_router_save_routes', 'model_router_test_route',
  'project_create', 'project_list', 'project_rename', 'memory_profile_read', 'memory_profile_save',
  'agent_save', 'agent_list', 'agent_delete', 'artifact_from_file', 'artifact_read', 'artifact_edit', 'artifact_list',
  'browser_view', 'browser_command', 'terminal_start', 'terminal_write', 'terminal_read', 'terminal_close', 'extend_command']

function commandDetail(row) {
  if (row.error_class === 'none' || row.stage === 'transport' || !probeCommands.includes(row.command) ||
      !['busy', 'unavailable', 'unauthorized', 'timeout', 'rejected'].includes(row.command_error_class)) return {}
  return { command: row.command, command_error_class: row.command_error_class }
}

const probePhases = ['chat', 'features', 'restart', 'update', 'update-restart']
const transportKinds = ['dns', 'connect', 'tls', 'tls_certificate', 'proxy', 'timeout', 'other']

function transportDetail(row) {
  if (row.stage !== 'transport' || row.transport !== 'failed' || row.error_class !== 'network' ||
      !transportKinds.includes(row.transport_kind)) return {}
  let host = null
  if (typeof row.host === 'string' && /^[A-Za-z0-9.:[\]-]{1,253}$/.test(row.host)) {
    try {
      const parsed = new URL(`https://${row.host}`)
      const domain = row.host.replace(/\.$/, '')
      if (parsed.hostname === row.host && (row.host.startsWith('[') || domain.split('.').every(label =>
        /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/.test(label)))) host = row.host
    } catch { /* Invalid hosts cannot appear in diagnostics. */ }
  }
  return { transport_kind: row.transport_kind, host }
}

function modelSaveDetail(row) {
  if (row.stage === 'model-save') {
    const rejected = row.transport === 'failed' ? true : row.transport === 'complete' ? false : null
    return { model_save_rejected: rejected,
      os_error: rejected && Number.isInteger(row.os_error) && row.os_error >= -2147483648 && row.os_error <= 2147483647 ? row.os_error : null }
  }
  if (row.stage !== 'inventory') return {}
  const identifier = '[A-Za-z0-9][A-Za-z0-9._-]{0,127}'
  const provider = new RegExp(`^${identifier}$`)
  const model = new RegExp(`^(?:${identifier}/)?[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`)
  const safe = (value, pattern) => value === null ? null
    : typeof value === 'string' && !/[\x00-\x1f\x7f]/.test(value) && pattern.test(value) ? value : '[redacted]'
  return { settings_read: row.settings_read === true,
    defaultProvider: safe(row.defaultProvider, provider), defaultModel: safe(row.defaultModel, model) }
}

export function readProbeProgress(env, transport = false) {
  try {
    const file = path.join(env.MUNIMENT_STATE_DIR, transport ? 'subscription-probe-transport-progress.jsonl' : 'subscription-probe-progress.jsonl')
    const stat = fs.lstatSync(file)
    if (!stat.isFile() || stat.size > 32_768) return []
    const fd = fs.openSync(file, 'r')
    let text
    try {
      const bytes = Buffer.alloc(32_769)
      const size = fs.readSync(fd, bytes, 0, bytes.length, 0)
      if (size > 32_768) return []
      text = bytes.subarray(0, size).toString('utf8')
    } finally { fs.closeSync(fd) }
    return text.slice(0, text.lastIndexOf('\n') + 1).split('\n').filter(Boolean).flatMap(line => {
      try {
        const row = JSON.parse(line)
        if (!probePhases.includes(row.phase) || !probeStages.includes(row.stage) ||
            !probeErrors.includes(row.error_class) || !['not-started', 'pending', 'accepted', 'complete', 'failed'].includes(row.transport) ||
            !(row.turn === null && row.requested === null || Number.isInteger(row.turn) && row.turn >= 0 && row.turn < 4 &&
              typeof row.requested === 'string' && /^[A-Za-z0-9._:-]{1,128}$/.test(row.requested))) return []
        return [{ phase: row.phase, stage: row.stage, turn: row.turn, requested: row.requested,
          transport: row.transport, error_class: row.error_class, ...transportDetail(row), ...modelSaveDetail(row), ...commandDetail(row) }]
      } catch { return [] }
    })
  } catch { return [] }
}

export function probeFailure(env) {
  const current = readProbeProgress(env).at(-1)
  const transport = readProbeProgress(env, true).filter(row => row.phase === current?.phase &&
    row.turn === current?.turn && row.requested === current?.requested).at(-1)
  // Only a provider refusal during the reply can block access. A later UI failure remains a product failure.
  if (current?.stage === 'reply' && transport?.transport === 'failed' && ['auth', 'quota'].includes(transport.error_class)) {
    return transport.error_class
  }
  return 'product'
}

export function probeProgress(env, step, phase) {
  try {
    const checkpoint = path.join(env.MUNIMENT_STATE_DIR, 'subscription-probe.json')
    const current = JSON.parse(fs.readFileSync(checkpoint, 'utf8')).phase
    if (['chat', 'features', 'restart', 'update', 'update-restart'].includes(current)) phase = current
  } catch { /* Keep the last phase when the checkpoint is missing or incomplete. */ }
  const progress = readProbeProgress(env).filter(row => row.phase === phase)
  const transports = readProbeProgress(env, true).filter(row => row.phase === phase)
  const current = progress.at(-1)
  const save = progress.filter(row => row.stage === 'model-save' && row.turn === current?.turn && row.requested === current?.requested).at(-1)
  const selection = current?.stage === 'inventory' ? {
    model_save_status: save?.transport || 'not-started',
    model_save_rejected: save?.model_save_rejected ?? null,
    os_error: save?.os_error ?? null,
  } : {}
  const transport = transports.filter(row => row.turn === current?.turn && row.requested === current?.requested).at(-1)
  return `step=${step}\nphase=${phase}` +
    progress.map(row => `\nprobe-progress=${JSON.stringify(row)}`).join('') +
    transports.map(row => `\nprovider-progress=${JSON.stringify(row)}`).join('') +
    (current ? `\nprobe-current=${JSON.stringify({ ...current, ...selection,
      provider_transport: transport?.transport || 'not-started', provider_error_class: transport?.error_class || 'none',
      ...(transport?.transport_kind ? { provider_transport_kind: transport.transport_kind, provider_host: transport.host } : {}) })}` : '')
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

// Use only the environment from isolatedEnvironment after disposable login validation.
// Read only named logs. Never walk credentials or conversations.
export function profileLogs(env, redact) {
  if (!env) return 'The disposable profile has not started.\n'
  const files = [
    ['app', path.join(env.TMPDIR, 'subscription-app.log')],
    ['runtime', path.join(env.TMPDIR, 'subscription-runtime.log')],
    ['msi-admin', path.join(env.TMPDIR, 'subscription-msi-admin.log')],
    ['cef', path.join(env.MUNIMENT_STATE_DIR, 'browser/cef.log')],
    ['keychain', path.join(env.MUNIMENT_STATE_DIR, 'browser/keychain-audit.log')],
    ['runtime-service', path.join(env.HOME, 'Library/Logs/Muniment/runtime-service.log'), env.HOME],
    ['runtime-native', path.join(env.LOCALAPPDATA, 'ai.muniment.desktop/logs/runtime.log'), env.LOCALAPPDATA],
  ]
  return files.map(([name, file, root = path.dirname(env.TMPDIR)]) => `${name} tail:\n${readDiagnosticLog(file, root, redact)}`).join('')
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
