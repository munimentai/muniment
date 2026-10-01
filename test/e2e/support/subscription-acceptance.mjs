import { createHash } from 'node:crypto'

export const platforms = ['linux', 'windows', 'macos-arm64', 'macos-x64']
export const chatFeatures = ['chat', 'direct-model-selection', 'model-switching']
export const featureChecks = {
  routing: ['fallback-selected', 'empty-sample-rejected', 'unavailable-model-excluded'],
  'account-balancing': ['multiple-accounts-served', 'equal-weight-shares', 'no-active-reservations'],
  settings: ['account-edit-saved', 'invalid-edit-rejected'],
  tools: ['file-tool-completed'],
  mcp: ['server-connected', 'tool-discovered', 'tool-completed'],
  files: ['file-read', 'file-edit-saved', 'stale-edit-rejected'],
  'restart-persistence': ['thread-restored', 'model-restored', 'file-restored', 'settings-restored'],
  'signed-update': ['signature-verified', 'tampered-package-rejected', 'signed-version-verified', 'busy-install-rejected', 'app-relaunched', 'profile-restored', 'candidate-digest-verified'],
  'local-startup': ['composer-visible', 'runtime-connected'],
  projects: ['project-created', 'project-renamed'],
  memory: ['profile-saved', 'profile-restored'],
  terminal: ['shell-output', 'shell-closed'],
  agents: ['agent-saved', 'agent-renamed', 'agent-deleted'],
  artifacts: ['html-published', 'artifact-read', 'artifact-renamed'],
  browser: ['local-page-rendered', 'unsafe-navigation-rejected', 'view-closed'],
}
export const features = [...chatFeatures, ...Object.keys(featureChecks)]
// Failure arrays share the installed probe's bounded string-list wire format.
export function featureFailure(feature, observed) {
  const stages = feature === 'mcp' ? ['check', 'server-add', 'connection-test', 'toggle', 'tool-turn', 'server-remove', 'receipt']
    : feature === 'artifacts' ? ['check', 'folders', 'new-file', 'save', 'publish', 'read', 'rename']
      : feature === 'signed-update' ? ['check', 'restore'] : ['check']
  if (!Object.hasOwn(featureChecks, feature) || !Array.isArray(observed) || observed.length !== 3 || observed[0] !== 'failed' ||
      !stages.includes(observed[1]) || !['check-failed', 'timeout'].includes(observed[2])) return {}
  return { failure_stage: observed[1], error_class: observed[2] }
}
const providers = { openai: 'openai-codex', anthropic: 'anthropic', xai: 'xai', kimi: 'kimi' }
const packageNames = {
  linux: /^muniment_\d+\.\d+\.\d+_amd64\.AppImage$/,
  windows: /^muniment_\d+\.\d+\.\d+_x64_en-US\.msi$/,
  'macos-arm64': /^muniment-arm64\.app\.tar\.gz$/,
  'macos-x64': /^muniment-x64\.app\.tar\.gz$/,
}
const modelId = /^[a-zA-Z0-9][a-zA-Z0-9._:-]{0,127}$/
const validModel = value => typeof value === 'string' && modelId.test(value)
const uuid = /^[a-f0-9]{8}-[a-f0-9]{4}-[1-8][a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/
const sha = /^[a-f0-9]{40}$/
const digest = /^[a-f0-9]{64}$/
export const hash = bytes => createHash('sha256').update(bytes).digest('hex')

export function checkIdentity(candidate, sourceSha, platform, bytes) {
  if (!sha.test(sourceSha) || candidate.source_sha !== sourceSha || candidate.platform !== platform || !platforms.includes(platform)) {
    throw new Error('The package source or platform does not match the candidate.')
  }
  if (!digest.test(candidate.sha256) || hash(bytes) !== candidate.sha256) {
    throw new Error('The package digest does not match the candidate.')
  }
  const prefix = `nightly-${sourceSha}-${platform.startsWith('macos-') ? 'macos' : platform}-`
  if (typeof candidate.asset !== 'string' || !candidate.asset.startsWith(prefix) || !/^[A-Za-z0-9._-]+$/.test(candidate.asset)) {
    throw new Error('The package name does not identify the candidate source.')
  }
  return candidate.asset.slice(prefix.length)
}

// Accept access-only leases. The factory keeps refresh tokens and refresh ownership.
export function subscriptionAccounts(models, leases, now = Date.now()) {
  if (!Array.isArray(models) || models.length !== 4 || new Set(models.map(m => m.id)).size !== 4 ||
      models.some(m => !m || Object.keys(m).some(key => !['family', 'id'].includes(key)) ||
        !Object.hasOwn(providers, m.family) || !validModel(m.id))) {
    throw new Error('Select four distinct supported subscription models.')
  }
  if (!Array.isArray(leases) || !leases.length) throw new Error('Provide fresh factory subscription access leases.')
  const families = [...new Set(models.map(model => model.family))]
  if (families.some(family => !leases.some(lease => lease?.provider === providers[family])) ||
      new Set(leases.map(lease => lease?.access)).size !== leases.length ||
      new Set(leases.filter(lease => lease?.account_id).map(lease => `${lease.provider}/${lease.account_id}`)).size !==
        leases.filter(lease => lease?.account_id).length ||
      leases.some(lease => !lease || !families.some(family => providers[family] === lease.provider))) {
    throw new Error('Provide distinct factory access leases for each selected subscription provider.')
  }
  return leases.map((lease, index) => {
    const family = families.find(family => providers[family] === lease.provider)
    if (Object.keys(lease).some(key => !['provider', 'access', 'expires_ms', 'account_id'].includes(key)) ||
        typeof lease.access !== 'string' || !lease.access || lease.access.length > 16384 || /\s/.test(lease.access) ||
        !Number.isSafeInteger(lease.expires_ms) || lease.expires_ms < now + 20 * 60_000 ||
        (lease.account_id !== undefined && (typeof lease.account_id !== 'string' || !/^[A-Za-z0-9_-]{1,128}$/.test(lease.account_id))) ||
        ((lease.provider === 'openai-codex' || leases.filter(item => item.provider === lease.provider).length > 1) && !lease.account_id)) {
      throw new Error('Provide access-only subscription leases valid for at least 20 minutes. Keep refresh tokens at the factory.')
    }
    return { id: `probe-${index}`, family, label: `Test subscription ${index + 1}`,
      credential: { type: 'subscription', ...lease }, models: models.filter(model => model.family === family).map(model => model.id), enabled: true, weight: 1 }
  })
}

export class AcceptanceError extends Error {
  constructor(condition) {
    super(`The subscription acceptance condition failed: ${condition}.`)
    this.condition = condition
  }
}

function requireCondition(passed, condition) {
  if (!passed) throw new AcceptanceError(condition)
}

// Naming can finish after chat starts and can return the chat nonce as its title.
// Exclude only its explicit request purpose, never a reply digest or arrival position.
export function chatTransports(receipts, firstModel) {
  if (!Array.isArray(receipts)) return receipts
  let named = false
  return receipts.filter(receipt => {
    if (receipt?.purpose === 'chat') return true
    requireCondition(!named && receipt?.purpose === 'thread-name' &&
      receipt.requested === firstModel && receipt.actual === firstModel &&
      receipt.subscription === true && receipt.finished === true && receipt.tools === 0 &&
      typeof receipt.reply_sha256 === 'string' && digest.test(receipt.reply_sha256), 'chat-transport-scope')
    named = true
    return false
  })
}

export function acceptance(candidate, sourceSha, platform, result, transports, packageName) {
  requireCondition(sha.test(sourceSha) && candidate?.source_sha === sourceSha, 'candidate-source')
  requireCondition(platforms.includes(platform) && candidate.platform === platform, 'candidate-platform')
  requireCondition(digest.test(candidate.sha256), 'candidate-digest')
  requireCondition(typeof packageName === 'string' && packageNames[platform].test(packageName), 'package-name')
  requireCondition(result?.status === 'passed', 'probe-status')
  requireCondition(result.installed === true, 'probe-installed')
  requireCondition(result.unchanged === true, 'probe-unchanged')
  requireCondition(result.webdriver === false, 'probe-webdriver')
  requireCondition(result.source_sha === sourceSha, 'probe-source')
  requireCondition(result.package_sha256 === candidate.sha256, 'probe-package-digest')
  requireCondition(Array.isArray(result.turns) && result.turns.length === 4, 'chat-turn-count')
  requireCondition(Array.isArray(transports) && transports.length === 4, 'chat-transport-count')
  const threads = new Set(), runs = new Set(), actual = new Set()
  const models = result.turns.map((turn, index) => {
    const transport = transports[index]
    requireCondition(turn?.index === index, 'turn-index')
    requireCondition(uuid.test(turn.thread), 'turn-thread')
    requireCondition(uuid.test(turn.run), 'turn-run')
    requireCondition(turn.rendered === true, 'turn-rendered')
    requireCondition(turn.context === true, 'turn-context')
    requireCondition(validModel(turn.requested), 'turn-model')
    requireCondition(transport?.requested === turn.requested, 'transport-requested-model')
    requireCondition(transport.actual === turn.requested, 'transport-actual-model')
    requireCondition(transport.finished === true, 'transport-finished')
    requireCondition(transport.subscription === true, 'transport-subscription')
    requireCondition(transport.tools === 0, 'transport-tools')
    requireCondition(typeof turn.expected === 'string' && /^MUNIMENT-[a-f0-9]{32}$/.test(turn.expected), 'turn-expected-reply')
    requireCondition(transport.reply_sha256 === hash(Buffer.from(turn.expected)), 'transport-reply-digest')
    threads.add(turn.thread)
    runs.add(turn.run)
    actual.add(transport.actual)
    return { subscription: true, requested: turn.requested, actual: transport.actual,
      reply: turn.expected, run_id: turn.run, thread_id: turn.thread }
  })
  requireCondition(threads.size === 1, 'chat-thread-continuity')
  requireCondition(runs.size === 4, 'chat-distinct-runs')
  requireCondition(actual.size === 4, 'chat-distinct-models')
  requireCondition(new Set(models.map(m => m.reply)).size === 1, 'chat-reply-continuity')
  return { schema: 1, source_sha: sourceSha, packages: { [packageName]: candidate.sha256 },
    cases: features.map(feature => {
      const checks = featureChecks[feature]
      const observed = result.features?.[feature]
      const passed = !checks || (Array.isArray(observed) && observed.length === checks.length &&
        checks.every((check, index) => observed[index] === check))
      return { platform, feature, status: passed ? 'passed' : 'blocked', installed: true,
        package_sha256: candidate.sha256, evidence: `${platform}-subscription.json`,
        models: chatFeatures.includes(feature) ? models : [],
        ...(checks ? { checks: passed ? [...checks] : [],
          ...(!passed ? { reason: 'The installed feature check did not finish. Read the native runner log.',
            ...featureFailure(feature, observed) } : {}) } : {}) }
    }) }
}

export function blocked(sourceSha, platform, reason) {
  return { schema: 1, source_sha: sourceSha, packages: {}, cases: features.map(feature => ({
    platform, feature, status: 'blocked', installed: false, evidence: `${platform}-subscription.json`, reason,
  })) }
}
