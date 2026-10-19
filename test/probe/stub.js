import { probeApprovalItems, probeNotifications, probeUsage } from './usage-fixture.js'
import { GROUP_THREAD, handleAgentRequest, probeAgentHistory, probeAgentStores, probeCreations, probeExtensions, probePosts } from './agents-fixture.js'
import { arrangeStandIn, toolsStandIn } from '../sidebar-standin.js'
import { advanceProjects, projectsAgents, projectsArrangement, projectsCatalog, projectsPalette, projectsRendered, projectsSettings, projectsSummaries, projectsThreadSettings } from './projects-fixture.js'
import { advanceTrust, trustAnswer, trustHistory, trustRendered } from './trust-fixture.js'
import { longThreadHistory } from './long-thread-fixture.js'
import { presentActivities } from './presentations.js'
import { advanceTools, toolApprovalItems, toolsHistory, toolsRendered } from './tools-fixture.js'
import { advanceFleetSync, fleetSyncAnswer, fleetSyncHistory, fleetSyncProjects, fleetSyncRendered, fleetSyncSummaries, fleetSyncThread } from './fleet-fixture.js'

export const historyFixtures = {
  'run-trust': trustHistory,
  tools: toolsHistory(),
  agents: [],
  projects: [],
  'fleet-sync': [],
  'signed-out': [],
  'runtime-exit': [],
  runtimes: [],
  onboarding: [],
  scan: [],
  access: [],
  empty: [],
  // Generated when the page loads, so other fixtures never copy it.
  'long-thread': [],
  'prompt-storage': [
    {
      runId: 'probe-prompt-storage',
      prompt: null,
      phase: 'complete',
      text: 'The local notes list the lease renewal date and notice period.',
      promptStorageNotice: 'Prompt text stays in runtime memory for this run. The keyring refused prompt history: Store(PlatformFailure(Error { code: -25307, message: "A default keychain could not be found." })).',
      receipt: { time: '0.8s' },
      toolActivity: [],
      resumable: false,
    },
  ],
  'local-mode': [
    {
      runId: 'probe-local-complete',
      prompt: 'Summarize the local notes.',
      phase: 'complete',
      text: 'The local notes list the lease renewal date and notice period.',
      receipt: { time: '0.8s' },
      toolActivity: [],
      resumable: false,
    },
  ],
  restored: [
    {
      runId: 'probe-complete',
      prompt: 'Find the renewal terms in the lease.',
      phase: 'complete',
      text: 'The lease renews for one year unless either party gives 60 days notice.',
      receipt: {
        route: 'analysis/high',
        model: 'openai/gpt-5.5',
        cost: '$0.014',
        time: '6.2s',
        capabilities: [{ name: 'files', version: '2' }],
      },
      recalls: [{ query: 'renewal terms', files: ['Documents/Muniment/lease.pdf'] }],
      toolActivity: [
        { effectId: 'probe-search', displayName: 'Search files', status: 'completed' },
        { effectId: 'probe-read', displayName: 'Read lease.pdf', status: 'completed' },
      ],
      resumable: false,
    },
    {
      runId: 'probe-interrupted',
      prompt: 'Draft a short renewal reminder.',
      phase: 'interrupted',
      text: 'Subject: Lease renewal notice\n\nThis is a reminder that',
      receipt: null,
      toolActivity: [],
      resumable: true,
    },
  ],
  'in-flight': [
    {
      runId: 'probe-settled',
      prompt: 'List the lease documents.',
      phase: 'complete',
      text: 'The archive holds three leases: Northwind, Ridgeway, and Halden.',
      receipt: {
        route: 'analysis/low',
        model: 'pi-2',
        cost: '$0.004',
        time: '1.8s',
      },
      toolActivity: [{ effectId: 'probe-list', displayName: 'List folder', status: 'completed' }],
      resumable: false,
    },
    {
      runId: 'probe-in-flight',
      prompt: 'Compare the renewal clauses across the three leases.',
      phase: 'streaming',
      text: 'Northwind renews for one year unless either party gives 60 days notice. Ridgeway carries the same window, and Halden',
      receipt: null,
      toolActivity: [
        { effectId: 'probe-read', displayName: 'read', status: 'completed', input: JSON.stringify({ path: '/leases/northwind.md' }), output: 'Renewal notice: 60 days.', startedAt: '2026-09-18T19:00:00Z', finishedAt: '2026-09-18T19:00:02Z' },
        { effectId: 'probe-compare', displayName: 'read', status: 'running', input: JSON.stringify({ path: '/leases/ridgeway.md' }), startedAt: new Date().toISOString() },
        { effectId: 'probe-search', displayName: 'grep', status: 'completed', input: JSON.stringify({ pattern: 'renewal', path: '/leases' }), output: '3 matching files' },
        { effectId: 'probe-command', displayName: 'bash', status: 'completed', input: JSON.stringify({ command: 'wc -l /leases/*.md', description: 'Count lease lines' }), output: '147 total' },
        { effectId: 'probe-log', displayName: 'bash', status: 'completed', input: JSON.stringify({ command: 'cat /leases/audit.log', description: 'Read the audit log' }), output: 'audit entry 1\naudit entry 2\n', outputBytes: 48_213, outputTruncated: true },
      ],
      resumable: false,
    },
  ],
  markdown: [
    {
      runId: 'probe-markdown',
      prompt: 'Summarize the release notes.',
      phase: 'complete',
      text: [
        '## Release notes',
        '',
        '- Added **terminal Markdown**',
        '- Kept `streaming` replies plain',
        '',
        '```text',
        'A fenced line that stays inside its own horizontally scrolling block.',
        '```',
        '',
        '| Surface | Result |',
        '| --- | --- |',
        '| Thread | [Ready with a deliberately wide table value](https://example.com) |',
      ].join('\n'),
      receipt: null,
      toolActivity: [],
      resumable: false,
    },
  ],
  // A composer thread in Ask before changes, paused on core's gate for a
  // command. `?state=menu` opens the composer's Modes menu, `?state=new`
  // opens it on a new thread, which starts in Bypass permissions,
  // `?state=plan` pauses it on a plan instead, and `?state=settings` opens
  // Settings → Permissions.
  'permission-modes': [
    {
      runId: 'probe-mode-gate',
      prompt: 'Clear out last year\'s exports.',
      phase: 'pending-permission',
      text: 'I will remove the exports from last year.',
      receipt: null,
      toolActivity: [],
      pendingPermission: {
        gateId: 'probe-mode-gate',
        kind: 'select',
        title: 'Allow bash `rm exports/2025-*.csv` in /Users/alice/Documents/muniment? This thread asks before it changes anything.',
        options: ['Allow once', 'Allow for this thread', 'Deny'],
      },
      resumable: false,
    },
  ],
  'pending-permission': [
    {
      runId: 'probe-permission',
      prompt: 'Delete the old export.',
      phase: 'pending-permission',
      text: 'I need permission before I continue.',
      receipt: null,
      toolActivity: [],
      pendingPermission: {
        gateId: 'probe-gate',
        kind: 'confirm',
        title: 'Delete a file',
        message: '/Documents/muniment/exports/old.csv',
      },
      resumable: false,
    },
  ],
  select: [
    {
      runId: 'probe-select',
      prompt: 'Export the renewal summary.',
      phase: 'pending-permission',
      text: 'Choose an export target before I continue.',
      receipt: null,
      toolActivity: [],
      pendingPermission: {
        gateId: 'probe-select-gate',
        kind: 'select',
        title: 'Export target',
        options: ['PDF', 'Word', 'Plain text', 'Markdown', 'Email draft', 'Clipboard'],
      },
      resumable: false,
    },
  ],
  input: [
    {
      runId: 'probe-input',
      prompt: 'Find the client folder.',
      phase: 'pending-permission',
      text: 'I need the folder name before I continue.',
      receipt: null,
      toolActivity: [],
      pendingPermission: {
        gateId: 'probe-input-gate',
        kind: 'input',
        title: 'Client folder name',
        placeholder: 'Example: Northwind',
      },
      resumable: false,
    },
  ],
  editor: [
    {
      runId: 'probe-editor',
      prompt: 'Archive the old export.',
      phase: 'pending-permission',
      text: 'Review the command before I continue.',
      receipt: null,
      toolActivity: [],
      pendingPermission: {
        gateId: 'probe-editor-gate',
        kind: 'editor',
        title: 'Archive command',
        prefill: 'mv exports/old.csv archive/old.csv',
      },
      resumable: false,
    },
  ],
  'code-diff': [
    {
      runId: 'probe-code-diff',
      prompt: 'Update the welcome message.',
      phase: 'pending-permission',
      text: 'Review the proposed changes before I continue.',
      receipt: null,
      toolActivity: [],
      pendingPermission: {
        gateId: 'probe-code-diff-gate',
        kind: 'code_diff',
        effect_id: 'probe-code-diff-effect',
        code_diff_id: 'fixture-modified',
        diff_sha256: 'probe-diff-hash',
        write_plan_sha256: 'probe-plan-hash',
      },
      resumable: false,
    },
  ],
  'code-diff-unavailable': [
    {
      runId: 'probe-code-diff-unavailable',
      prompt: 'Update the welcome message.',
      phase: 'pending-permission',
      text: 'Review the proposed changes before I continue.',
      receipt: null,
      toolActivity: [],
      pendingPermission: {
        gateId: 'probe-code-diff-unavailable-gate',
        kind: 'code_diff',
        effect_id: 'probe-code-diff-unavailable-effect',
        code_diff_id: 'fixture-unavailable',
        diff_sha256: 'probe-diff-hash',
        write_plan_sha256: 'probe-plan-hash',
      },
      resumable: false,
    },
  ],
  'applied-diff': [
    {
      runId: 'probe-applied-diff',
      prompt: 'Update the welcome message.',
      phase: 'complete',
      text: 'I updated the welcome message.',
      receipt: null,
      toolActivity: [],
      appliedDiffs: [
        { effectId: 'probe-applied-effect', codeDiffId: 'fixture-modified' },
        { effectId: 'probe-unavailable-effect', codeDiffId: 'fixture-unavailable' },
      ],
      resumable: false,
    },
  ],
}


// First-run provider scans. `?scan=` picks one: mac (the default), many or none.
const omlx = { id: 'omlx', name: 'oMLX', baseUrl: 'http://127.0.0.1:8080/v1', state: 'needs-key', modelCount: 8, defaultModel: 'Qwen3-Coder-30B-A3B-Instruct-4bit', keyFile: '~/.omlx/settings.json' }
// ?accounts: an Anthropic pool with two subscriptions and a key, to show the account cards.
function probeAccounts() {
  const hour = 3600 * 1000
  const base = { family: 'anthropic', enabled: true, servable: true, weight: 1, models: [], days: [], allowance_readable: true, quota_observed_ms: Date.now(), requests: 0, input_tokens: 0, output_tokens: 0, active: 0, errors: 0 }
  const windows = (left) => [
    { label: '5-hour', scope: '', remaining_percent: left, resets_at_ms: Date.now() + 3 * hour, limit_reached: false },
    { label: 'Weekly', scope: '', remaining_percent: 98, resets_at_ms: Date.now() + 160 * hour, limit_reached: false },
  ]
  return {
    families: [{ id: 'anthropic', name: 'Anthropic', base_url: 'https://api.anthropic.com/v1' }],
    subscriptions: [{ provider: 'anthropic', family: 'anthropic', label: 'Claude Pro or Max' }],
    accounts: [
      { ...base, id: 'team', label: 'team', source: 'subscription', windows: windows(89) },
      { ...base, id: FLEET_PROBE ? `shared:swarm:${FLEET_STUDIO}:personal` : 'personal', label: 'personal', source: 'subscription', windows: windows(100) },
      { ...base, id: 'work', label: 'Work', source: 'key', overflow: false, allowance_readable: false, windows: [] },
    ],
  }
}

// ?accounts&fleet opens Settings → Models on a fleet: this machine
// holds the team sign-in and Studio holds the personal one, which offers Hold
// sign-in here. Studio also holds a Codex sign-in it shares, still on its
// way, and Pod a Claude sign-in it keeps to itself. Core answers
// `fleet.credentials` and `fleet.hold_sign_in`.
const FLEET_PROBE = new URLSearchParams(location.search).has('fleet')
const FLEET_LAPTOP = '018f0000-0000-7000-8000-000000000041'
const FLEET_STUDIO = '018f0000-0000-7000-8000-000000000042'
const FLEET_POD = '018f0000-0000-7000-8000-000000000046'
const fleetAccount = (name, label, holder, holderName, extra = {}) => ({ name, provider: 'anthropic', family: 'anthropic', kind: 'subscription', label, refresher: true, enabled: true, holder, holder_name: holderName, shared: true, this_machine: holder === FLEET_LAPTOP, usable_here: true, ...extra })
function fleetAnswer({ operation }) {
  if (operation === 'fleet.hold_sign_in') return { status: 'requested', holder: FLEET_STUDIO }
  return {
    share_models: true, use_vault: false, role: 'full', removed: false, secrets: [], audit: [], vault: [],
    accounts: [
      fleetAccount('router:team', 'team', FLEET_LAPTOP, 'Laptop'),
      fleetAccount('pi:anthropic', 'anthropic', FLEET_POD, 'Pod', { kind: 'oauth', shared: false, usable_here: false }),
      fleetAccount('router:personal', 'personal', FLEET_STUDIO, 'Studio'),
      fleetAccount('pi:openai-codex', 'openai-codex', FLEET_STUDIO, 'Studio', { provider: 'openai-codex', family: undefined, kind: 'oauth', usable_here: false }),
    ],
  }
}
const fleetRendered = () => document.querySelectorAll('[data-hold]').length === 2 && !!document.querySelector('[data-hold] button') && document.body.textContent.includes('Signed in on Studio, arriving')
function advanceFleetFixture() {
  if (!FLEET_PROBE || fleetRendered()) return
  if (!document.querySelector('.settings-layout')) return document.querySelector('.sidebar-actions button[aria-label="Settings"][aria-expanded="false"]')?.click()
  const providers = Array.from(document.querySelectorAll('#sidebar nav[aria-label="Settings sections"] button')).find((button) => button.textContent.trim() === 'Models')
  if (providers && providers.getAttribute('aria-current') !== 'true') providers.click()
}

const providerScanFixtures = {
  mac: {
    servers: [omlx],
    hints: [
      { provider: 'anthropic', evidence: 'Claude Code is signed in', reuse: true },
      { provider: 'openai', evidence: 'Codex CLI is signed in', reuse: false },
      { provider: 'github-copilot', evidence: 'Copilot CLI is installed', reuse: false },
    ],
  },
  many: {
    servers: [
      { id: 'ollama', name: 'Ollama', baseUrl: 'http://127.0.0.1:11434/v1', state: 'ready', modelCount: 3, defaultModel: 'qwen3:8b' },
      omlx,
      { id: 'lmstudio', name: 'LM Studio', baseUrl: 'http://127.0.0.1:1234/v1', state: 'not-running', modelCount: 0 },
    ],
    hints: [{ provider: 'anthropic', evidence: 'Claude Code is signed in', reuse: true }],
  },
  none: { servers: [], hints: [] },
}

// The runtime's web search settings in memory, shaped like the
// `search_` model actions answer them. A key keeps only its last four
// characters.
const SEARCH_ENGINES = ['brave', 'bing', 'duckduckgo', 'yahoo', 'wikipedia']
const SEARCH_BASE_URL = new Set(['exa', 'brave', 'tavily', 'firecrawl', 'gemini', 'degoog'])
const SEARCH_PROVIDERS = [
  'exa', 'brave', 'tavily', 'firecrawl', 'perplexity', 'serper', 'serpapi', 'kagi', 'jina', 'parallel', 'you', 'openai',
  'gemini', 'tinyfish', 'search1api', 'searchinfinity', 'querit', 'serpdive', 'bocha', 'ollama', 'anysearch', 'xcrawl',
  'xai', 'mistral', 'brightdata', 'serpbase', 'serply', 'valyu', 'baizhi', 'zai', 'keenable', 'ceramic', 'degoog',
].map((id) => ({ id, base_url: SEARCH_BASE_URL.has(id), extra: id === 'brightdata' ? ['brightdataSerpZone', 'brightdataUnlockerZone'] : [] }))

function probeSearch() {
  let order = [{ id: 'muniment', enabled: true }, { id: 'tavily', enabled: true }, { id: 'exa', enabled: true }]
  let engines = [...SEARCH_ENGINES]
  const keys = new Map([['tavily', { last4: '3f2a' }]])
  const serves = (id) => id === 'muniment' || id === 'exa' || keys.has(id)
  const view = () => ({
    order: order.map(({ id, enabled }) => ({ id, enabled, has_key: keys.has(id) })),
    engines: SEARCH_ENGINES.map((id) => ({ id, enabled: engines.includes(id) })),
    keys: [...keys].map(([provider, key]) => ({ provider, has_key: true, ...key })),
    providers: structuredClone(SEARCH_PROVIDERS),
    running: true,
  })
  return {
    web_search_settings: () => view(),
    web_search_save_order: ({ order: next }) => {
      const unknown = next.find(({ id }) => !serves(id))
      if (unknown) throw new Error(`${unknown.id} has no key, so it cannot join the order.`)
      order = next.map(({ id, enabled }) => ({ id, enabled }))
      return view()
    },
    web_search_save_engines: ({ engines: next }) => {
      if (!next.length) throw new Error('Turn on at least one engine, or turn off Muniment search in the order.')
      engines = SEARCH_ENGINES.filter((id) => next.includes(id))
      return view()
    },
    web_search_set_key: ({ provider, apiKey, baseUrl, extra }) => {
      if (!SEARCH_PROVIDERS.some(({ id }) => id === provider)) throw new Error(`${provider} is not a search provider that takes a key.`)
      const key = String(apiKey ?? '').trim()
      if (!key) throw new Error('The key is empty.')
      keys.set(provider, {
        ...(key.length > 8 ? { last4: key.slice(-4) } : {}),
        ...(baseUrl ? { base_url: baseUrl } : {}),
        ...(extra && Object.values(extra).some(Boolean) ? { extra: Object.fromEntries(Object.entries(extra).filter(([, value]) => value)) } : {}),
      })
      if (!order.some(({ id }) => id === provider)) order.push({ id: provider, enabled: true })
      return view()
    },
    web_search_remove_key: ({ provider }) => {
      if (!keys.delete(provider)) throw new Error(`${provider} has no key.`)
      order = order.filter(({ id }) => serves(id))
      return view()
    },
  }
}

// The per-file counts a chat event carries in place of an applied diff.
function diffSummary(diff) {
  return {
    files: diff.files.map((file) => {
      const lines = file.hunks.flatMap((hunk) => hunk.lines)
      return { oldPath: file.oldPath, newPath: file.newPath, status: file.status, binary: file.binary,
        additions: lines.filter((line) => line.kind === 'addition').length, deletions: lines.filter((line) => line.kind === 'deletion').length }
    }),
    truncated: diff.truncated,
  }
}

export function buildProbeCommandTable(fixtureName) {
  const fixtureHistory = historyFixtures[fixtureName]
  if (!fixtureHistory) throw new Error(`Unknown probe history fixture: ${fixtureName}`)
  // The permission-modes fixture's `?state=new` starts on a new thread.
  const newThread = fixtureName === 'permission-modes' && new URLSearchParams(location.search).get('state') === 'new'
  const history = newThread ? [] : fixtureName === 'long-thread' ? longThreadHistory() : structuredClone(fixtureHistory)
  // The bodies the stub answers through chat_run_detail.
  const runDetails = { appliedDiff: null }
  if (fixtureName === 'applied-diff') history[0].toolActivity = [{ effectId: 'preview-read', displayName: 'read', status: 'completed', input: JSON.stringify({ path: 'src/welcome.js' }) }]
  // Each call carries core's presentation, as the runtime sends it.
  for (const entry of history) presentActivities(entry.toolActivity)
  const onboardingFixture = fixtureName === 'onboarding'
  const scanFixture = fixtureName === 'scan'
  const accessFixture = fixtureName === 'access'
  const signedOutFixture = fixtureName === 'signed-out'
  // `runtimes.html?state=` picks local, paired, remote, switcher, pairing,
  // failed, ssh, pairing-off, pairing-on, link or devices. Every other
  // fixture holds this computer's runtime alone.
  const runtimesFixture = fixtureName === 'runtimes'
  const runtimesState = runtimesFixture ? new URLSearchParams(location.search).get('state') ?? 'paired' : 'local'
  // `agents.html?state=` picks list, profile, cron, webhook, mcp, proposals,
  // notifications, machines, move, transcript, group-create, meeting, approvals,
  // approval-inline, replays, import, absence, preferences, empty,
  // notifications-empty, usage, usage-tokens, usage-cost, usage-partial or
  // composer-limit, with agents on three machines that all answer and a
  // synced copy from a fourth. `cards` opens the catalog's action cards, and
  // `cards-offline` the same with the Studio machine offline.
  // `fleet-sync.html?state=` shows a fleet this laptop reaches only through
  // the swarm: the sync line, owners, a queued post, a replica's history, a
  // new thread on a container, the offline scope, the bootstrap and the log.
  const fleetSyncFixture = fixtureName === 'fleet-sync'
  if (fleetSyncFixture && fleetSyncThread()) history.push(...fleetSyncHistory())
  const agentsFixture = fixtureName === 'agents'
  const projectsFixture = fixtureName === 'projects'
  const trustFixture = fixtureName === 'run-trust'
  const toolsFixture = fixtureName === 'tools'
  const agentsState = agentsFixture ? new URLSearchParams(location.search).get('state') ?? 'list' : null
  let runtimeList = probeRuntimes(agentsFixture ? (agentsState === 'cards-offline' ? 'agents-offline' : 'agents') : runtimesState)
  let localAccess = probeLocalAccess(['pairing-on', 'link', 'devices'].includes(runtimesState))
  let localDevices = runtimesState === 'devices' ? probeDevices() : []
  const onboardingHomePath = '/Users/alice/Documents/muniment'
  // The runtimes fixture lists a thread on each machine, so the sidebar
  // shows the machine badges.
  const threadSummaries = fleetSyncFixture
    ? fleetSyncSummaries()
    : projectsFixture
    ? projectsSummaries()
    : agentsFixture
    ? [
        { threadId: 'probe-local', title: 'Lease renewal', updatedAt: '2026-07-28T11:55:00Z', runtimeId: 'local' },
        { threadId: 'thread-a2', title: 'Inbox triage', updatedAt: '2026-07-28T11:00:00Z', runtimeId: 'local' },
        { threadId: 'thread-a2b', title: 'Vendor follow-up', updatedAt: '2026-07-27T11:00:00Z', runtimeId: 'local' },
        { threadId: 'thread-a3', title: 'Release editor', updatedAt: '2026-07-28T10:30:00Z', runtimeId: '018f0000-0000-7000-8000-000000000042' },
        { threadId: 'thread-a4', title: 'Nightly test triage', updatedAt: '2026-07-28T10:00:00Z', runtimeId: '018f0000-0000-7000-8000-000000000043' },
        { threadId: 'probe-box', title: 'Nightly test notes', updatedAt: '2026-07-27T10:00:00Z', runtimeId: '018f0000-0000-7000-8000-000000000043' },
        { threadId: GROUP_THREAD, title: 'Launch room', updatedAt: '2026-07-28T11:30:00Z', runtimeId: 'local' },
        { threadId: 'thread-a5', title: 'Front desk', updatedAt: '2026-07-26T10:00:00Z', runtimeId: 'local' },
      ]
    : runtimesFixture
    ? [
        { threadId: 'probe-local', title: 'Lease renewal', updatedAt: '2026-07-28T11:55:00Z', runtimeId: 'local' },
        { threadId: 'probe-box', title: 'Nightly test triage', updatedAt: '2026-07-28T10:00:00Z', runtimeId: '018f0000-0000-7000-8000-000000000043' },
        { threadId: 'probe-studio', title: 'Draft board memo', updatedAt: '2026-07-27T09:00:00Z', runtimeId: '018f0000-0000-7000-8000-000000000042' },
      ]
    : history.length
    ? [
        { threadId: 'probe-thread', title: 'Lease renewal', updatedAt: '2026-07-28T11:55:00Z' },
        { threadId: 'probe-archive', title: 'Archive review', updatedAt: '2026-07-28T09:00:00Z' },
        { threadId: 'probe-notes', title: 'Client notes', updatedAt: '2026-07-25T12:00:00Z' },
      ]
    : []
  const olderThreadSummaries = history.length
    ? [{ threadId: 'probe-older', title: 'Older correspondence', updatedAt: '2026-07-20T12:00:00Z' }]
    : []
  let profileMemory = '# Profile\n\n## Preferred name\n\nAlex\n\n## Instructions\n\nUse clear, concise answers.\n'
  // ?memories shows one saved fact of each trust level. ?memories=200 shows
  // that many varied facts and a few deleted ones. Both start with Ask before
  // saving on and four suggested changes waiting for review.
  const memoryCount = Number(new URLSearchParams(location.search).get('memories')) || 0
  const memorySeed = new URLSearchParams(location.search).has('memories')
  let recallSettings = memorySeed ? { learn: true, use: true, ask: true } : null
  let savedFacts = memoryCount ? probeFacts(memoryCount) : new URLSearchParams(location.search).has('memories') ? [
    { id: 'f1', title: 'Preferred editor', content: 'Uses Helix for code.', source: 'Added in settings', trust: 'user' },
    { id: 'f2', title: 'Report style', content: 'Lead with the answer, then the evidence.', source: 'thread:t1 run:r1', trust: 'conversation' },
    { id: 'f3', title: 'Vendor contact', content: 'Send invoices to the address on the vendor site.', source: 'thread:t2 run:r2', trust: 'external' },
  ] : []
  let deletedFacts = memoryCount ? probeFacts(4, 'd').map((fact) => ({ ...fact, title: `Old ${fact.title.toLowerCase()}` })) : []
  let pendingChanges = memorySeed ? probePending(savedFacts) : []
  const agentStores = probeAgentStores(agentsFixture, agentsState)
  if (projectsFixture) Object.assign(agentStores.local, projectsAgents())
  const modesFixture = fixtureName === 'permission-modes'
  const modesState = modesFixture ? new URLSearchParams(location.search).get('state') ?? 'gate' : null
  // The gate thread was set to Ask before changes; new threads start in
  // the default, Bypass permissions.
  if (modesFixture) agentStores.local.threadModes = { 'probe-thread': 'manual' }
  if (modesState === 'plan') {
    const plan = ['1. List the exports in exports/ by year.', '2. Move the 2025 files to archive/2025/ instead of deleting them.', '3. Report the files moved and the space freed.'].join('\n')
    history[0] = { ...history[0], prompt: 'Plan how to clear out last year\'s exports.', text: 'Here is the plan.', pendingPermission: { gateId: 'probe-plan-gate', kind: 'select', title: 'Approve this plan?', options: ['Approve, then ask before changes', 'Approve and edit automatically', 'Approve with auto review', 'Keep planning'] } }
    agentStores.local.threadModes = { 'probe-thread': 'plan' }
    agentStores.local.plans = { 'probe-thread': { run_id: 'probe-mode-gate', gate_id: 'probe-plan-gate', approval_id: 'probe-mode-gate:probe-plan-gate', summary: '1. List the exports in exports/ by year.', plan, answers: ['plan_manual', 'plan_edit_auto', 'plan_auto', 'plan_keep'] } }
  }
  const agentMemory = {}
  const projects = { 'project-lease': 'Lease renewal', 'project-research': 'Research' }
  const projectThreads = { 'probe-thread': 'project-lease', 'probe-archive': 'project-lease', 'probe-notes': 'project-research' }
  const searchSettings = probeSearch()
  const eventListeners = []
  const invokedCommands = []
  const unknownCommands = []
  let currentThreadId = fleetSyncFixture ? fleetSyncThread() : history.length ? 'probe-thread' : null
  let retentionChoice = null
  let runtimeState = fixtureName === 'runtime-exit'
    ? { revision: 1, lastEvent: 'exited', visible: true, busy: false }
    : { revision: 0, lastEvent: 'connected', visible: false, busy: false }

  // `?context=caution`, `unknown` or `estimated` shows that meter state.
  let probeContext = new URLSearchParams(location.search).get('context') || 'normal'
  const contextUsage = () => ({
    normal: { tokens: 92400, contextWindow: 200000, percent: 46.2, estimated: false, windowKnown: true },
    caution: { tokens: 170000, contextWindow: 200000, percent: 85, estimated: false, windowKnown: true },
    unknown: { tokens: 21480, contextWindow: 128000, percent: 16.8, estimated: false, windowKnown: false },
    estimated: { tokens: 61900, contextWindow: 200000, percent: 30.95, estimated: true, windowKnown: true },
  })[probeContext] ?? null

  async function invoke(command, payload) {
    // `?update=0.3.0` offers that version as a ready update.
    if (command === 'app_update_prepare') return new URLSearchParams(location.search).get('update')
    if (command === 'app_update_interruptions') return 0
    if (command === 'thread_context_usage') {
      const usage = contextUsage()
      return usage && { ...usage, provider: probeContext === 'unknown' ? 'ollama' : 'anthropic', model: probeContext === 'unknown' ? 'qwen3:8b' : 'claude-sonnet', autoCompact: true, compactAt: usage.contextWindow - 16384, compacting: false }
    }
    if (command === 'thread_compact') {
      await new Promise((resolve) => setTimeout(resolve, 1500))
      probeContext = 'estimated'
      return { tokensBefore: 152400, estimatedTokensAfter: 31900 }
    }
    if (command === 'launcher_register') return null
    if (command === 'artifact_list') return []
    if (command === 'creation_list') return JSON.parse(JSON.stringify(probeCreations))
    // The plan arrives as the app's live state, which only serializes as JSON.
    if (command === 'creation_save') {
      const plan = JSON.parse(JSON.stringify(payload.creation))
      const kept = probeCreations.find((item) => item.threadId === plan.threadId)
      if (!plan.turns?.length && kept?.turns) plan.turns = kept.turns
      probeCreations.splice(0, probeCreations.length, ...probeCreations.filter((item) => item.threadId !== plan.threadId), plan)
      return plan
    }
    if (command === 'extend_command' && payload?.action === 'read') return agentsFixture ? structuredClone(probeExtensions) : { items: [] }
    if (command === 'browser_view') return null
    if (command === 'runtime_state') return runtimeState
    if (command === 'runtime_start') {
      runtimeState = { revision: 2, lastEvent: 'connected', visible: false, busy: false }
      for (const entry of eventListeners.filter(({ event }) => event === 'runtime-state-changed')) {
        entry.listener({ payload: runtimeState })
      }
      return null
    }
    if (command === 'local_mode_status') return ['local-mode', 'prompt-storage', 'projects'].includes(fixtureName)
    if (command === 'local_mode_enter') return null
    if (command === 'local_mode_leave') return null
    if (command === 'local_mode_provider_inventory') return {
      providers: [
        ...(new URLSearchParams(location.search).has('accounts') ? [{ id: 'anthropic', name: 'Anthropic', source: 'key', base_url: null, models: [] }] : []),
        { id: 'google', name: 'Google', source: 'key', base_url: null, models: [{ id: 'gemini-3-pro', context: '1M', max_out: '64K', thinking: true, images: true, thinking_levels: ['low', 'high'], thinking_default: 'high' }] },
        { id: 'ollama', name: 'Ollama', source: 'local', base_url: 'http://localhost:11434/v1', models: [{ id: 'llama3.2:3b', context: '128K', max_out: '16.4K', thinking: false, images: false, thinking_levels: [], thinking_default: null }, { id: 'qwen3:8b', context: '40K', max_out: '8K', thinking: true, images: false, thinking_levels: ['off', 'minimal', 'low', 'medium', 'high'], thinking_default: 'medium' }] },
      ],
      // `?thinking` starts on a model with thinking levels, so the composer shows its level control.
      default_provider: 'ollama',
      default_model: new URLSearchParams(location.search).has('thinking') ? 'qwen3:8b' : 'llama3.2:3b',
      hidden: [],
    }
    if (['local_mode_store_provider_key', 'local_mode_set_default_model', 'local_mode_set_model_hidden', 'local_mode_set_thinking_level', 'local_mode_disconnect_provider', 'local_mode_connect_claude_code', 'local_mode_account_login_start', 'local_mode_account_login_answer', 'local_mode_account_login_cancel', 'local_mode_open_url'].includes(command)) return null
    if (command === 'local_mode_store_endpoint') return 'custom-endpoint'
    if (command === 'model_registry') return (await fetch('./fixtures/model-registry.json')).json()
    if (command === 'local_mode_claude_code_status') return { installed: true, logged_in: true, path: '/usr/local/bin/claude' }
    if (command === 'context_settings') return { enabled: true, reserveTokens: 16384, keepRecentTokens: 20000 }
    if (command === 'context_settings_save') return payload
    if (command === 'chat_search_files') return [{ path: '/Documents/muniment/ISSUES.md', relativePath: 'ISSUES.md', displayName: 'ISSUES.md' }].filter((file) => file.relativePath.toLowerCase().includes(payload.query.toLowerCase()))
    if (command === 'chat_file_metadata') return { displayName: 'ISSUES.md', byteLength: 128, mediaType: 'text/markdown' }
    if (command === 'chat_file_content') return 'export function welcome(name) {\n  // The current file from the workspace.\n  return `Welcome, ${name}`\n}\n'
    if (command === 'home_status') {
      if (onboardingFixture) return { configured: false, homePath: onboardingHomePath }
      return { configured: true, homePath: '/Documents/muniment' }
    }
    if (command === 'home_confirm') return { configured: true, homePath: payload.homePath }
    if (command === 'onboarding_connect_server') return null
    if (command === 'onboarding_provider_scan') return structuredClone(providerScanFixtures[new URLSearchParams(location.search).get('scan') ?? 'mac'] ?? providerScanFixtures.mac)
    if (command === 'onboarding_scan') return {
      findings: [
        { assistantId: 'claude-code', displayName: 'Claude Code', root: '/Users/alice/.claude', fileCount: 12, byteTotal: 1024, capped: false, warnings: [] },
        { assistantId: 'pi', displayName: 'Pi', root: '/Users/alice/.pi/agent', fileCount: 3, byteTotal: 512, capped: true, warnings: ['timeCap'] },
      ],
      errors: [],
    }
    if (command === 'memory_import_assistant') return {
      importedFileCount: 9,
      alreadyCopiedCount: 2,
      leftOut: [{ sourceName: 'rules/keys.md', reason: 'secret' }],
      destination: `memory/imports/${payload.assistantId}`,
    }
    if (command === 'plugin:dialog|open' && scanFixture) return '/Users/alice/Downloads/claude-export.zip'
    if (command === 'onboarding_import_preview') return {
      entries: [
        { name: 'memory.md', kind: 'markdown', byteSize: 2048, excerpt: '', excerptTruncated: false },
        { name: 'conversations.json', kind: 'json', byteSize: 4096, excerpt: '', excerptTruncated: false },
      ],
      totalByteSize: 6144,
    }
    if (command === 'onboarding_import_extract') return payload.selectedNames.map((name) => ({
      sourceName: name,
      kind: name.endsWith('.json') ? 'json' : 'markdown',
      text: 'Preview text.\n',
      sourceProvenance: `assistant-export-zip:v1:probe:${name}`,
    }))
    if (command === 'home_confirm_import') return { configured: true, importedFileCount: payload.approvedEntries.length, alreadyCopiedCount: 0 }
    if (command === 'auth_status') {
      if (signedOutFixture) return { signed_in: false, subject: null }
      return { signed_in: true, subject: 'probe-user' }
    }
    if (command === 'chat_thread_summaries') {
      if (payload.cursor === 'older') return { summaries: structuredClone(olderThreadSummaries), nextCursor: null }
      return { summaries: structuredClone(threadSummaries), nextCursor: history.length ? 'older' : null }
    }
    if (command === 'chat_current_thread') return currentThreadId
    if (command === 'chat_select_thread') {
      if (![...threadSummaries, ...olderThreadSummaries].some(({ threadId }) => threadId === payload.threadId)) {
        throw new Error('The preview thread does not exist.')
      }
      currentThreadId = payload.threadId
      return null
    }
    if (command === 'model_router_settings' || command === 'model_router_refresh_quota') return {
      enabled: false, running: false, is_default: false, base_url: null,
      ...(new URLSearchParams(location.search).has('accounts') ? probeAccounts() : { accounts: [], subscriptions: [], families: [] }), options: [], routes: [],
      fallback: null, min_confidence: 0.6, served_models: [],
      classifier: { kind: 'none', configured: false, model: '' },
    }
    if (Object.hasOwn(searchSettings, command)) return searchSettings[command](payload ?? {})
    if (command === 'memory_profile_read') return profileMemory
    if (command === 'memory_profile_save') { profileMemory = payload.content; return null }
    if (command === 'memory_recall_settings') return recallSettings && { ...recallSettings }
    if (command === 'memory_recall_settings_save') { recallSettings = { ...payload.settings }; return { ...recallSettings } }
    if (command === 'memory_facts') return structuredClone(savedFacts)
    if (command === 'memory_fact_save') {
      const fact = { ...payload.fact, id: payload.fact.id || crypto.randomUUID(), trust: 'user' }
      savedFacts = [...savedFacts.filter(f => f.id !== fact.id), fact]; return fact
    }
    if (command === 'memory_fact_delete') { deletedFacts.push(...savedFacts.filter(f => f.id === payload.id)); savedFacts = savedFacts.filter(f => f.id !== payload.id); return null }
    if (command === 'memory_deleted_facts') return structuredClone(deletedFacts)
    if (command === 'memory_fact_restore') { savedFacts.push(...deletedFacts.filter(f => f.id === payload.id)); deletedFacts = deletedFacts.filter(f => f.id !== payload.id); return null }
    // Suggested changes wait only while Ask before saving is on. The one
    // marked stale fails to apply, as a proposal does when its fact changed.
    if (command === 'memory_pending') return recallSettings?.ask ? structuredClone(pendingChanges.map(({ stale, ...proposal }) => proposal)) : []
    if (command === 'memory_pending_apply') {
      const proposal = pendingChanges.find(p => p.id === payload.id)
      if (!proposal) throw 'This suggested change is no longer waiting.'
      pendingChanges = pendingChanges.filter(p => p.id !== payload.id)
      if (proposal.stale) throw 'This memory changed after the suggestion. Review it again.'
      if (proposal.op === 'delete') { deletedFacts.push(...savedFacts.filter(f => f.id === proposal.fact_id)); savedFacts = savedFacts.filter(f => f.id !== proposal.fact_id); return null }
      const fact = { ...(proposal.before || { id: crypto.randomUUID(), source: proposal.source }), ...proposal.after, trust: 'conversation' }
      savedFacts = proposal.op === 'add' ? [...savedFacts, fact] : savedFacts.map(f => f.id === fact.id ? fact : f)
      return structuredClone(fact)
    }
    if (command === 'memory_pending_dismiss') { pendingChanges = pendingChanges.filter(p => p.id !== payload.id); return null }
    if (command === 'record_report') return { report: { open: 0, findings: [], counts: {} } }
    if (command === 'agent_memory') {
      const data = agentMemory[payload.id] ||= { facts: [], deleted: [] }
      if (payload.action === 'memory_facts') return structuredClone(data.facts)
      if (payload.action === 'memory_deleted_facts') return structuredClone(data.deleted)
      if (payload.action === 'memory_pending') return []
      if (payload.action === 'memory_fact_save') { const fact = JSON.parse(JSON.stringify({ ...payload.fact, id: payload.fact.id || crypto.randomUUID() })); data.facts = [...data.facts.filter(item => item.id !== fact.id), fact]; return fact }
      if (payload.action === 'memory_fact_delete') { data.deleted.push(...data.facts.filter(item => item.id === payload.factId)); data.facts = data.facts.filter(item => item.id !== payload.factId); return null }
      if (payload.action === 'memory_fact_restore') { data.facts.push(...data.deleted.filter(item => item.id === payload.factId)); data.deleted = data.deleted.filter(item => item.id !== payload.factId); return null }
    }
    if (command === 'agent_export_template') return true
    // Core's `settings.scope` answers from its protocol fixture on every page.
    if (command === 'agent_request' && payload?.operation === 'settings.scope') {
      return fetch('/core/protocol-fixtures/muniment.attach/1/response-settings-scope.json').then((response) => response.json()).then((fixture) => fixture.body)
    }
    if (FLEET_PROBE && command === 'agent_request' && payload?.operation?.startsWith('fleet.')) return fleetAnswer(payload)
    if (fleetSyncFixture && command === 'agent_request') {
      const answer = fleetSyncAnswer(payload)
      if (answer !== undefined) return answer
    }
    if (trustFixture && command === 'agent_request') {
      const answer = trustAnswer(payload, history)
      if (answer !== undefined) return answer
    }
    if (command === 'agent_request') return handleAgentRequest(agentStores, payload)
    // Usage and Notifications answer from the agents fixture's states; every
    // other page reads a quiet fleet, so the composer shows no limit.
    if (command === 'usage_summary') {
      const usage = probeUsage(payload, agentsState)
      return agentsState?.startsWith('usage') || agentsState === 'composer-limit' ? usage : { ...usage, limits: [], unmetered: [] }
    }
    if (command === 'notifications_list') {
      const page = probeNotifications(payload, agentsState)
      if (agentsState === 'approvals') return { ...page, needs_you: [...probeApprovalItems(), ...page.needs_you.filter((item) => item.kind !== 'approval')] }
      if (agentsState === 'tool-approvals') return { ...page, needs_you: toolApprovalItems(probeApprovalItems()[0].approval) }
      return agentsFixture ? page : { ...page, needs_you: [], running: [], days: [], counts: { needs_you: 0, running: 0, failed: 0, unread: 0 } }
    }
    if (command === 'agent_open') return null
    if (command === 'sidebar_arrange') return projectsFixture ? projectsArrangement(payload.input) : arrangeStandIn(payload.input)
    // The palette answers core's ranking of the projects fixture.
    if (command === 'palette_rank') return projectsPalette(payload.input)
    if (command === 'project_tools') return toolsStandIn(payload.input)
    if (command === 'project_move_thread') {
      const catalog = projectsFixture ? projectsCatalog : { threads: projectThreads }
      if (!payload.projectId) delete catalog.threads[payload.threadId]
      else catalog.threads[payload.threadId] = payload.projectId
      return null
    }
    // Each thread keeps its own model; the default model never changes.
    if (command === 'thread_settings') return projectsThreadSettings(payload.threadId)
    if (command === 'thread_settings_set') return projectsThreadSettings(payload.threadId, payload)
    if (command === 'project_settings') return projectsFixture ? projectsSettings(payload.projectId) : { instructions: '' }
    if (command === 'project_settings_save') return null
    if (command === 'project_list' && projectsFixture) return structuredClone(projectsCatalog)
    if (command === 'project_list' && fleetSyncFixture) return structuredClone(fleetSyncProjects)
    if (command === 'project_list') return { projects: { ...projects }, threads: { ...projectThreads } }
    if (command === 'project_create') {
      if (Object.values(projects).includes(payload.name)) throw new Error('A project with that name already exists.')
      const id = crypto.randomUUID(); projects[id] = payload.name; return id
    }
    if (command === 'project_rename') { projects[payload.projectId] = payload.name; return null }
    if (command === 'project_open') return null
    if (command === 'chat_new_thread') {
      const local = agentStores.local
      if (payload?.agentId && local.primaryThreads[payload.agentId]) { currentThreadId = local.primaryThreads[payload.agentId]; return currentThreadId }
      currentThreadId = crypto.randomUUID()
      const projectId = payload?.projectId || local.agents.find(a => a.id === payload?.agentId)?.projectId
      if (projectId) projectThreads[currentThreadId] = projectId
      if (payload?.agentId) local.primaryThreads[payload.agentId] = currentThreadId
      threadSummaries.unshift({ threadId: currentThreadId, title: 'New thread', updatedAt: new Date().toISOString() })
      return currentThreadId
    }
    if (command === 'chat_rename_thread') {
      const summary = [...threadSummaries, ...olderThreadSummaries].find(({ threadId }) => threadId === payload.threadId)
      if (summary) summary.title = payload.title
      return null
    }
    if (command === 'chat_delete_thread') {
      const index = threadSummaries.findIndex(({ threadId }) => threadId === payload.threadId)
      if (index !== -1) threadSummaries.splice(index, 1)
      if (currentThreadId === payload.threadId) currentThreadId = null
      return null
    }
    if (command === 'chat_submit') {
      const runId = 'probe-new-run'
      currentThreadId = 'probe-new-thread'
      // Core sets the new thread's mode before the run starts: the picked
      // one, else the default.
      const modes = handleAgentRequest(agentStores, { runtime: 'local', operation: 'permission.modes' })
      agentStores.local.threadModes = { ...agentStores.local.threadModes, [currentThreadId]: payload.permissionMode ?? modes.default_thread_mode }
      threadSummaries.unshift({
        threadId: currentThreadId,
        title: payload.prompt,
        updatedAt: new Date().toISOString(),
      })
      queueMicrotask(() => {
        for (const entry of eventListeners.filter(({ event }) => event === 'chat-event')) {
          entry.listener({ payload: { runId, type: 'completed', receipt: null } })
        }
      })
      return {
        runId,
        attachments: [
          { displayName: 'site-photo.png', byteLength: 18432, mediaType: 'image/png' },
          { displayName: 'lease.pdf', byteLength: 219136 },
        ],
      }
    }
    if (command === 'chat_answer_permission') return null
    if (command === 'chat_thread_open' && agentsFixture) return { entries: probeAgentHistory(payload.threadId), posted: probePosts(agentStores, payload.threadId), nextCursor: null }
    // Pages follow the runtime's: oldest first, `limit` entries each, and a cursor while more remain.
    if (command === 'chat_thread_open') {
      const from = Number(payload?.cursor ?? 0)
      const until = payload?.limit ? from + payload.limit : history.length
      return { entries: structuredClone(history.slice(from, until)), nextCursor: until < history.length ? String(until) : null }
    }
    // Chat events leave out tool output bodies and applied diffs. These come from the run's record.
    if (command === 'chat_run_detail') {
      if (payload.kind === 'code_diff' && payload.effectId === 'probe-applied-effect' && runDetails.appliedDiff) return { kind: 'code_diff', diff: structuredClone(runDetails.appliedDiff) }
      if (payload.kind === 'tool_output' && payload.effectId === 'probe-log') {
        const text = Array.from({ length: 2_000 }, (_, index) => `audit entry ${index + 1}`).join('\n')
        return { kind: 'tool_output', text, bytes: text.length, truncated: false }
      }
      throw 'Conversation history is unavailable.'
    }
    if (command === 'auth_entitlement_snapshot') {
      return {
        snapshot_version: 2,
        subject: 'probe-user',
        user_display_name: 'Alice',
        org_id: 'probe-org',
        organization_display_name: 'Acme',
        role: 'owner',
        territory: 'us',
        capabilities: [],
        grants: [],
      }
    }
    if (command === 'auth_pairing_status') return { pair: null }
    if (command === 'auth_devices') return accessFixture
      ? [
          {
            device_id: 'probe-current-device',
            client_id: 'muniment-desktop',
            client_role: 'desktop',
            platform: 'desktop',
            created_at: '2026-07-01T12:00:00Z',
            revoked_at: null,
            last_active_at: '2026-08-13T12:00:00Z',
            current: true,
          },
          {
            device_id: 'probe-revoked-device',
            client_id: 'muniment-mobile',
            client_role: 'mobile',
            platform: 'ios',
            created_at: '2026-06-01T12:00:00Z',
            revoked_at: '2026-08-01T12:00:00Z',
            last_active_at: '2026-07-31T12:00:00Z',
            current: false,
          },
        ]
      : []
    if (command === 'thread_retention_choice') return retentionChoice
    if (command === 'record_thread_retention_choice') {
      retentionChoice = payload.choice
      return null
    }
    if (command === 'attach_listener_status') return { started: true, failure: null, connected: onboardingFixture, supervisor_running: false }
    // The record panel opens on one company whose kind list runs past the
    // viewport, so the shell's scroll and title row can be checked with it open.
    if (command === 'record_companies') return { companies: [{ id: 'probe-company', name: 'Northwind', created_at: '2026-01-01T00:00:00.000Z', owner_principal_id: 'probe-owner', current: true }], current: 'probe-company' }
    if (command === 'record_kinds') return {
      company_id: 'probe-company',
      kinds: ['person', 'org', 'deal', 'thread', 'message', 'ticket', 'task', 'project', 'document', 'meeting', 'subscription', 'invoice', 'service', 'incident', 'deploy', 'commitment', 'decision', 'mapping', 'workflow', 'view', 'x_region', 'x_territory', 'x_quota', 'x_plan']
        .map((name, index) => ({ name, count: index * 7, schema: { properties: { name: { type: 'string' } }, required: ['name'] }, states: null, extension: null })),
      relations: [],
    }
    if (command === 'record_query') return { page: { kind: payload?.kind ?? 'org', total: 0, offset: 0, limit: 200, sort: 'updated_at', descending: true, rows: [] } }
    if (command === 'attach_companions') return [
      {
        identity: '018f0000-0000-7000-8000-000000000001',
        claimed_kind: 'CLI',
        claimed_version: '1.2.3',
        approved_at: '2026-08-04T12:00:00Z',
      },
    ]
    if (command === 'attach_revoke_companion') return null
    if (command === 'runtime_list' || command === 'runtime_refresh') return structuredClone(runtimeList)
    if (command === 'local_access_settings') return structuredClone(localAccess)
    if (command === 'local_access_save') {
      localAccess = probeLocalAccess(payload.enabled, payload.endpoints)
      return structuredClone(localAccess)
    }
    if (command === 'local_pairing_link') return { link: PROBE_LOCAL_LINK, qrSvg: probeQrSvg(), expiresAt: new Date(Date.now() + 10 * 60_000).toISOString(), scopes: payload.admin ? ['run.write', 'runtime.admin', 'runtime.update', 'thread.read'] : ['run.write', 'runtime.update', 'thread.read'] }
    if (command === 'local_devices') return { devices: structuredClone(localDevices) }
    if (command === 'local_device_revoke') {
      localDevices = localDevices.filter((device) => device.device_id !== payload.deviceId)
      return true
    }
    if (command === 'runtime_select') {
      runtimeList = { ...runtimeList, selected: payload.id, runtimes: runtimeList.runtimes.map((runtime) => ({ ...runtime, active: runtime.id === payload.id })) }
      return structuredClone(runtimeList)
    }
    if (command === 'runtime_pair_link' || command === 'runtime_pair_ssh') {
      if (runtimesState === 'failed') throw 'The runtime refused the link. It may be used or expired. Create a new link.'
      // Pairing stays in progress, so the fixture shows its state line.
      return new Promise(() => {})
    }
    if (command === 'runtime_rename') {
      runtimeList = { ...runtimeList, runtimes: runtimeList.runtimes.map((runtime) => runtime.id === payload.id ? { ...runtime, name: payload.name } : runtime) }
      return structuredClone(runtimeList)
    }
    if (command === 'runtime_remove') {
      runtimeList = { ...runtimeList, runtimes: runtimeList.runtimes.filter((runtime) => runtime.id !== payload.id) }
      return structuredClone(runtimeList)
    }
    if (command === 'runtime_update') {
      const runtime = runtimeList.runtimes.find((entry) => entry.id === payload.id)
      return { id: payload.id, name: runtime?.name ?? '', ok: true, version: runtimeList.appVersion, message: `Update to ${runtimeList.appVersion} requested.` }
    }
    if (command === 'runtime_update_all') {
      const paired = runtimeList.runtimes.filter((runtime) => !runtime.local)
      return {
        results: paired.filter((runtime) => runtime.updateOffered && runtime.status === 'online').map((runtime) => ({ id: runtime.id, name: runtime.name, ok: true, version: runtimeList.appVersion, message: `Update to ${runtimeList.appVersion} requested.` })),
        skipped: paired.filter((runtime) => !(runtime.updateOffered && runtime.status === 'online')).map((runtime) => ({ id: runtime.id, name: runtime.name, reason: runtime.status === 'online' ? 'current' : 'offline' })),
      }
    }
    if (command === 'voice_input_devices') return ['MacBook Pro Microphone', 'USB Audio Device']
    if (command === 'parakeet_install_facts') return { totalDownloadBytes: 482_000_000, requiredFreeBytes: 1_100_000_000 }
    if (command === 'parakeet_install_status') return { state: 'installed' }
    if (command === 'installed_fonts') return ['Avenir Next', 'Helvetica Neue', 'Iowan Old Style', 'Menlo', 'SF Mono']
    if (command === 'browser_language' || command === 'browser_language_set') return { languages: [{ code: 'en-US', state: 'installed' }, { code: 'de', state: 'installed' }, { code: 'fr', state: 'available' }, { code: 'ja', state: 'available' }], selected: payload?.code ?? null, active: null, downloadSize: 3_400_000 }
    unknownCommands.push(command)
    throw new Error(`Unknown probe command: ${command}`)
  }

  return {
    invoke,
    fleetSyncFixture,
    history,
    runDetails,
    agentsFixture,
    agentsState,
    projectsFixture,
    trustFixture,
    toolsFixture,
    modesFixture,
    modesState,
    onboardingFixture,
    scanFixture,
    accessFixture,
    signedOutFixture,
    runtimesFixture,
    runtimesState,
    onboardingHomePath,
    threadSummaries,
    olderThreadSummaries,
    eventListeners,
    invokedCommands,
    unknownCommands,
  }
}

const PROBE_LOCAL_LINK = `muniment://pair?v=1&id=018f0000-0000-7000-8000-000000000041&name=Laptop&fp=${'7a'.repeat(32)}&host=192.0.2.20%3A7443&host=100.64.0.7%3A7443#${'4d'.repeat(16)}`

// This computer's remote access: off, or on with a local network address, a
// private overlay network address and an address the user named.
function probeLocalAccess(enabled, configured = []) {
  return {
    settings: { enabled, listen: null, endpoints: configured },
    overridden: false,
    runtime_id: '018f0000-0000-7000-8000-000000000041',
    runtime_name: 'Laptop',
    certificate_sha256: '7a'.repeat(32),
    listening: enabled ? '[::]:7443' : null,
    endpoints: enabled
      ? configured.length
        ? configured.map((endpoint) => ({ endpoint, kind: 'configured' }))
        : [{ endpoint: '192.0.2.20:7443', kind: 'lan' }, { endpoint: '100.64.0.7:7443', kind: 'overlay' }, { endpoint: '[2001:db8::20]:7443', kind: 'public' }]
      : [],
    error: null,
  }
}

function probeDevices() {
  const at = (minutes) => new Date(Date.now() - minutes * 60_000).toISOString()
  return [
    { device_id: '018f0000-0000-7000-8000-000000000051', name: 'Studio', kind: 'desktop-client', scopes: ['run.write', 'thread.read'], created_at: at(9000), last_seen_at: at(12) },
    { device_id: '018f0000-0000-7000-8000-000000000052', name: 'Build box', kind: 'desktop-client', scopes: ['run.write', 'runtime.admin', 'thread.read'], created_at: at(20000), last_seen_at: null },
  ]
}

// A QR-shaped stand-in: three finder squares and a fixed module pattern. The
// app draws the real code from the link.
function probeQrSvg() {
  const size = 29
  let path = ''
  const finder = (x, y) => x < 7 && y < 7 || x >= size - 7 && y < 7 || x < 7 && y >= size - 7
  for (let y = 0; y < size; y += 1) {
    for (let x = 0; x < size; x += 1) {
      const dark = finder(x, y)
        ? (Math.max(Math.abs((x % (size - 7)) - 3), Math.abs((y % (size - 7)) - 3)) !== 2)
        : ((x * 7 + y * 13 + x * y) % 5 < 2)
      if (dark) path += `M${x + 4},${y + 4}h1v1h-1z`
    }
  }
  const width = size + 8
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${width}" shape-rendering="crispEdges" role="img" aria-label="Pairing code"><rect width="${width}" height="${width}" fill="white"/><path fill="black" d="${path}"/></svg>`
}

// This computer, then a desktop that is offline and an older container this
// device may update over its SSH tunnel.
function probeRuntimes(state) {
  const local = { id: 'local', name: 'This computer', kind: 'local', version: '0.2.0', status: 'online', local: true, scopes: [], admin: true, updateOffered: false, sshHost: null }
  const paired = [
    { id: '018f0000-0000-7000-8000-000000000042', name: 'Studio', kind: 'desktop', version: '0.2.0', status: state === 'agents' ? 'online' : 'offline', local: false, scopes: ['run.write', 'thread.read'], admin: false, updateOffered: false, sshHost: null },
    // The container reports the update it installs.
    { id: '018f0000-0000-7000-8000-000000000043', name: 'Build box', kind: 'container', version: '0.1.0', status: 'online', local: false, scopes: ['run.write', 'runtime.admin', 'thread.read'], admin: true, behind: true, mayUpdate: true, updateOffered: true, updateTarget: '0.2.0', updateCapability: 'launcher', updateStatus: { state: 'installing', targetVersion: '0.2.0', blockers: [] }, sshHost: 'build-box' },
  ]
  const selected = state === 'remote' ? paired[1].id : 'local'
  return {
    appVersion: '0.2.0',
    deviceName: 'Laptop',
    selected,
    runtimes: (state === 'local' ? [local] : [local, ...paired]).map((runtime) => ({ ...runtime, active: runtime.id === selected })),
  }
}

const browserFixtureScript = typeof document === 'undefined'
  ? null
  : document.querySelector('script[src$="stub.js"][data-history]')
if (browserFixtureScript) {
const fixtureName = browserFixtureScript.dataset.history
const probeTable = buildProbeCommandTable(fixtureName)
let callbackId = 0
const {
  fleetSyncFixture,
  history,
  runDetails,
  agentsFixture,
  agentsState,
  projectsFixture,
  trustFixture,
  toolsFixture,
  modesFixture,
  modesState,
  onboardingFixture,
  scanFixture,
  accessFixture,
  signedOutFixture,
  runtimesFixture,
  runtimesState,
  onboardingHomePath,
  threadSummaries,
  olderThreadSummaries,
  eventListeners,
  invokedCommands,
  unknownCommands,
} = probeTable
const PROBE_PAIRING_LINK = `muniment://pair?v=1&id=018f0000-0000-7000-8000-000000000044&name=Lab&fp=${'3c'.repeat(32)}&host=192.0.2.20%3A7443#${'5e'.repeat(16)}`
let probePairClicked = false

function recordInvoke(surface, command, payload) {
  invokedCommands.push({ surface, command, payload })
}

function runtimesRendered() {
  // Each row's meta line names the machine its thread lives on. A thread on
  // a paired runtime names it in its title row, and a new thread's machine
  // menu lists every machine.
  if (runtimesState === 'remote' || runtimesState === 'switcher') {
    const machines = Array.from(document.querySelectorAll('#sidebar .thread-row-meta .machine-name')).map((name) => name.textContent)
    if (!machines.includes('Build box')) return false
    if (runtimesState === 'remote') return !!document.querySelector('.thread-title-heading')?.textContent.includes('Nightly test triage') && !!document.querySelector('header .machine-badge')?.textContent.includes('Build box')
    const menu = document.querySelector('.machine-menu')
    return !!menu?.textContent.includes('Build box') && !!document.querySelector('button[data-menu="machine"]')?.getAttribute('aria-label')?.includes('This computer')
  }
  // Settings opens on all machines, with the foot row on its side and
  // Settings marked as the current page there.
  if (runtimesState === 'scope') {
    return document.querySelector('#settings-machine')?.value === 'all'
      && !!document.querySelector('.machine-scope .scope-note')
      && !!document.querySelector('#sidebar nav[aria-label="Settings sections"]')
      && !!document.querySelector('#sidebar .sidebar-actions button[aria-label="Settings"][aria-current="page"]')
  }
  const page = document.querySelector('.settings-runtimes')
  if (!page) return false
  const accept = page.querySelector('[role="switch"][aria-label="Accept connections"]')
  const show = Array.from(page.querySelectorAll('.add-machine button')).some((button) => button.textContent.trim() === 'Show code')
  if (runtimesState === 'pairing-off') return accept?.getAttribute('aria-checked') === 'false' && !accept.disabled && show
  if (runtimesState === 'pairing-on') return page.textContent.includes('Private overlay network')
  if (runtimesState === 'link') return !!page.querySelector('.add-machine .qr svg') && page.textContent.includes('expires in')
  const rows = page.querySelectorAll('.group-card > [role="group"]').length
  if (runtimesState === 'devices') return rows === 3 && !!page.querySelector('[aria-label="More for Build box"]') && page.textContent.includes('Limited')
  if (runtimesState === 'local') return rows === 1
  if (runtimesState === 'paired') return rows === 3 && page.textContent.includes('Offline') && page.textContent.includes('Updating to v0.2.0')
  if (runtimesState === 'pairing') return page.textContent.includes('Connecting…')
  if (runtimesState === 'failed') return !!page.querySelector('[role="alert"]')?.textContent.includes('refused the link')
  if (runtimesState === 'ssh') return page.querySelector('input[placeholder="user@host"]')?.value === 'me@build-box'
  return false
}

// The agents fixture opens the catalog, an agent's profile, one routine's
// editor, Notifications, Usage, Settings → Machines, a dialog or a conversation, as its
// state names.
const AGENT_STATES = {
  list: { agent: null },
  profile: { agent: 'Lead scout' },
  cron: { agent: 'Lead scout', routine: 'Weekday briefing' },
  webhook: { agent: 'Lead scout', routine: 'Build finished' },
  mcp: { agent: 'Lead scout', routine: 'New leads' },
  proposals: { agent: 'Inbox triage' },
  move: { agent: 'Front desk' },
  transcript: { agent: 'Inbox triage' },
  notifications: { agent: null },
  approvals: { agent: null },
  'tool-approvals': { agent: null },
  usage: { agent: null },
  'usage-tokens': { agent: null },
  'usage-cost': { agent: null },
  'usage-partial': { agent: null },
  'composer-limit': { agent: 'Lead scout' },
  machines: { agent: null },
  'group-create': { agent: null },
  meeting: { agent: null },
  import: { agent: null },
  absence: { agent: null },
  replays: { agent: 'Lead scout' },
  'approval-inline': { agent: 'Inbox triage' },
  preferences: { agent: null },
  cards: { agent: null },
  'cards-offline': { agent: null },
  empty: { agent: null },
  'notifications-empty': { agent: null },
  builder: { agent: null },
  'builder-question': { agent: null },
  'builder-thread': { agent: null },
}

const AGENT_SCREENS = {
  list: () => document.querySelectorAll('[data-panel="agents"] .machine-mark').length === 5 && document.querySelectorAll('#sidebar .project-title[aria-label^="Project "]').length >= 5 && ['running', 'waiting', 'idle', 'error'].every((state) => document.querySelector(`[data-panel="agents"] .agent-avatar[data-activity="${state}"]`)),
  notifications: () => document.querySelectorAll('[data-panel="notifications"] .notification-row').length >= 8 && document.querySelectorAll('[data-panel="notifications"] .approval').length === 3 && !!document.querySelector('[data-panel="notifications"] .needs[data-kind="limit"]'),
  approvals: () => document.querySelectorAll('[data-panel="notifications"] .approval').length === 5,
  'tool-approvals': () => document.querySelectorAll('[data-panel="notifications"] .approval .tool-call-card').length === 4,
  'notifications-empty': () => document.querySelector('[data-panel="notifications"]')?.textContent.includes('Nothing needs you') && document.querySelectorAll('[data-panel="notifications"] .upcoming li').length === 3,
  usage: () => document.querySelectorAll('[data-panel="usage"] .limit-card').length === 10 && !!document.querySelector('[data-panel="usage"] .unmetered'),
  'usage-tokens': () => document.querySelectorAll('[data-panel="usage"] .usage-chart .column').length === 7 && document.querySelectorAll('[data-panel="usage"] .breakdown li').length === 5,
  'usage-cost': () => document.querySelectorAll('[data-panel="usage"] .usage-chart .column').length === 30 && document.querySelector('[data-panel="usage"] .tiles')?.textContent.includes('API estimate'),
  'usage-partial': () => document.querySelector('[data-panel="usage"]')?.textContent.includes('Build box did not answer') && document.querySelectorAll('[data-panel="usage"] .usage-chart .column').length === 7,
  'composer-limit': () => !!document.querySelector('.limit-banner') && document.querySelector('.titlebar')?.textContent.includes('Lead scout'),
  'approval-inline': () => !!document.querySelector('.thread .approval.inline') && !!document.querySelector('.thread .silent-note'),
  replays: () => document.querySelectorAll('aside[aria-label="Agent profile"] [aria-label="Failed runs"] .replay').length === 2,
  preferences: () => !!document.querySelector('[aria-label="Agent avatars"]') && document.querySelectorAll('[aria-label="Agent avatar preview"] .agent-avatar svg').length === 4,
  empty: () => !!document.querySelector('[data-panel="agents"] .starter') && !document.querySelector('[data-panel="agents"] .agent-card'),
  cards: () => cardsRendered(5),
  'cards-offline': () => cardsRendered(5) && !!document.querySelector('[data-agent-card="018f0000-0000-4000-8000-0000000000a3"].read-only [role="note"]'),
  machines: () => !!document.querySelector('.settings-runtimes [aria-label="Office mini"]') && document.querySelector('.settings-runtimes [aria-label="Office mini"]')?.textContent.includes('Offline'),
  'group-create': () => document.querySelector('[data-panel="group"]')?.querySelectorAll('input[type="checkbox"]:checked').length === 3,
  meeting: () => document.querySelector('.thread')?.textContent.includes('Debate round 1') && document.querySelectorAll('.thread .round').length === 3,
  import: () => !!document.querySelector('[data-panel="import"] [aria-label="Skills"]'),
  absence: () => !!document.querySelector('#sidebar [aria-label="Routines paused"]'),
  transcript: () => !!document.querySelector('.thread [aria-label="Message from Lead scout"]') && !!document.querySelector('.thread .silent-note'),
  move: () => !!document.querySelector('[data-panel="move"] [role="note"]') && !!document.querySelector('[data-panel="move"] input:checked'),
  builder: () => !!document.querySelector('.thread [aria-label="Draft: Inbox brief"]') && !document.querySelector('.agent-chat [role="status"]'),
  'builder-question': () => !!document.querySelector('.thread [aria-label="Draft: Helper"] [aria-label="Question"]'),
  'builder-thread': () => !!document.querySelector('.thread [aria-label="Draft: Release notes"]'),
}

// The builder states open a new agent chat and describe an agent in one
// message, or vaguely, or make one from a thread.
const BUILDER_WORDS = { builder: 'Every weekday at 9am, summarize my inbox and list what needs a reply.', 'builder-question': 'Make me an agent' }
const probeBuildersTyped = new WeakSet()
let probeBuilderOpened = 0

// `&panel=overview|model|permissions|routines|activity` opens that panel on
// every card, and `&edit=name` opens the first card's name field.
const cardOptions = new URLSearchParams(location.search)
const CARD_PANEL = cardOptions.get('panel')
const CARD_EDIT = cardOptions.get('edit') === 'name'

function cardsRendered(count) {
  const cards = Array.from(document.querySelectorAll('[data-panel="agents"] .agent-card'))
  if (cards.length !== count) return false
  if (CARD_PANEL && !cards.every((card) => card.querySelector(`[role="tabpanel"][data-panel-id="${CARD_PANEL}"]`))) return false
  if (CARD_PANEL === 'activity' && cards.some((card) => card.textContent.includes('Loading runs'))) return false
  if (CARD_EDIT && !cards[0].querySelector('input[aria-label="Agent name"]')) return false
  return cards.every((card) => card.querySelector('.agent-avatar svg'))
}

function agentsRendered() {
  // A state that opens an agent's chat waits for the Agents page to give way to it.
  const agent = AGENT_STATES[agentsState]?.agent
  if (agent && (document.querySelector('[data-panel="agents"]') || !document.querySelector('aside[aria-label="Agent profile"]')?.textContent.includes(agent))) return false
  if (AGENT_SCREENS[agentsState]) return AGENT_SCREENS[agentsState]()
  const profile = document.querySelector('aside[aria-label="Agent profile"]')
  if (!profile?.textContent.includes(AGENT_STATES[agentsState].agent) || !profile.querySelector('[aria-label="Permissions"]')) return false
  if (agentsState === 'proposals') return profile.querySelectorAll('.proposal').length === 2
  const routine = AGENT_STATES[agentsState].routine
  if (!routine) return !!profile.querySelector('.runs .run')
  const editor = document.querySelector('[data-panel="routine"]')
  if (!editor?.querySelector('input')?.value.includes(routine)) return false
  if (agentsState === 'mcp') return editor.querySelectorAll('[aria-label^="Check"]').length >= 2
  if (agentsState === 'webhook') return !!editor.querySelector('[aria-label="Webhook address"] .secret code')
  return true
}

const buttonNamed = (root, name) => Array.from(root?.querySelectorAll('button') ?? []).find((button) => button.textContent.trim() === name || button.getAttribute('aria-label') === name)
// Opens a page through the sidebar's Search, one step per render: the words,
// then the page, once, since the page loads before it shows.
const probePagesOpened = new Set()
function openPage(label) {
  if (probePagesOpened.has(label)) return
  const search = document.querySelector('#sidebar input[aria-label="Search threads"]')
  if (!search) return
  if (search.value !== label) return typeInto(search, label)
  const page = buttonNamed(document.querySelector('#sidebar [aria-label="Search results"]'), label)
  if (!page) return
  probePagesOpened.add(label)
  page.click()
}
const probeImportRead = new WeakSet()
let probeHistoryLoaded = false
let probeGroupPicked = false
let probeCardsOpened = false
let probeChatOpened = false

function advanceAgentsFixture() {
  if (!agentsFixture || fixtureRendered()) return
  // The first history load selects a thread, so the driver waits for it.
  probeHistoryLoaded ||= !!document.querySelector('#sidebar .thread-row-title')
  if (!probeHistoryLoaded) return
  const target = AGENT_STATES[agentsState] ?? AGENT_STATES.list
  if (agentsState === 'absence') return
  if (agentsState === 'preferences') {
    if (!document.querySelector('.settings-layout')) return document.querySelector('.sidebar-actions button[aria-label="Settings"][aria-expanded="false"]')?.click()
    const appearance = Array.from(document.querySelectorAll('#sidebar nav[aria-label="Settings sections"] button')).find((button) => button.textContent.trim() === 'General')
    if (appearance && appearance.getAttribute('aria-current') !== 'true') return appearance.click()
    document.querySelector('[aria-label="Agent avatars"]')?.scrollIntoView({ block: 'center' })
    return
  }
  if (agentsState === 'empty') {
    if (!document.querySelector('[data-panel="agents"]')) openPage('Agents')
    return
  }
  if (agentsState === 'machines') {
    if (!document.querySelector('.settings-layout')) return document.querySelector('.sidebar-actions button[aria-label="Settings"][aria-expanded="false"]')?.click()
    const machines = Array.from(document.querySelectorAll('#sidebar nav[aria-label="Settings sections"] button')).find((button) => button.textContent.trim() === 'Machines')
    if (machines && machines.getAttribute('aria-current') !== 'true') machines.click()
    return
  }
  if (agentsState === 'group-create' || agentsState === 'meeting') {
    const dialog = document.querySelector('[data-panel="group"]')
    if (dialog) {
      if (probeGroupPicked) return
      probeGroupPicked = true
      const name = dialog.querySelector('input:not([type="checkbox"])')
      typeInto(name, 'Pricing review')
      for (const box of Array.from(dialog.querySelectorAll('input[type="checkbox"]')).slice(0, 3)) box.click()
      return
    }
    if (agentsState === 'group-create') return openPage('New group chat')
    return Array.from(document.querySelectorAll('#sidebar .thread-row')).find((row) => row.querySelector('.thread-row-title')?.textContent.trim() === 'Launch room')?.click()
  }
  if (agentsState.startsWith('builder')) {
    const chat = document.querySelector('#composer-message[placeholder^="Describe your agent"]')
    if (agentsState === 'builder-thread') {
      if (document.querySelector('.agent-chat')) return
      const menuItem = Array.from(document.querySelectorAll('.thread-menu [role="menuitem"]')).find((item) => item.textContent.trim() === 'Make this an agent')
      if (Date.now() - probeBuilderOpened < 1500) return
      probeBuilderOpened = Date.now()
      setTimeout(advanceAgentsFixture, 1600)
      if (menuItem) return menuItem.click()
      return document.querySelector('#sidebar button[aria-label^="Actions for"]')?.click()
    }
    // A press while the chat opens would open another, so the driver presses
    // again only after a pause, and sends into each chat once.
    if (!chat) {
      if (document.querySelector('.agent-chat article') || Date.now() - probeBuilderOpened < 1500) return
      const catalog = document.querySelector('[data-panel="agents"]')
      if (!catalog) return openPage('Agents')
      const create = buttonNamed(catalog, 'New agent')
      if (create && !create.disabled) { probeBuilderOpened = Date.now(); setTimeout(advanceAgentsFixture, 1600); create.click() }
      return
    }
    if (probeBuildersTyped.has(chat)) return
    probeBuildersTyped.add(chat)
    setTimeout(() => {
      typeInto(chat, BUILDER_WORDS[agentsState])
      setTimeout(() => buttonNamed(document, 'Send')?.click(), 50)
    }, 300)
    return
  }
  if (agentsState === 'cards' || agentsState === 'cards-offline') {
    // Agents opens once: a second click would mount the catalog again and reset its cards.
    const catalog = document.querySelector('[data-panel="agents"]')
    if (!catalog) {
      if (!probeCardsOpened) openPage('Agents')
      probeCardsOpened ||= document.querySelector('#sidebar input[aria-label="Search threads"]')?.value === 'Agents' && !!buttonNamed(document.querySelector('#sidebar [aria-label="Search results"]'), 'Agents')
      return
    }
    const cards = Array.from(catalog.querySelectorAll('.agent-card'))
    if (CARD_PANEL) for (const card of cards) {
      const tab = Array.from(card.querySelectorAll('[role="tab"]')).find((button) => button.getAttribute('aria-label').toLowerCase() === CARD_PANEL)
      if (tab?.getAttribute('aria-selected') === 'false') tab.click()
    }
    if (CARD_EDIT && cards.length && !cards[0].querySelector('input[aria-label="Agent name"]')) cards[0].querySelector('.head-name button')?.click()
    return
  }
  if (agentsState === 'list' || agentsState === 'import') {
    const catalog = document.querySelector('[data-panel="agents"]')
    if (!catalog) return openPage('Agents')
    if (agentsState !== 'import') return
    const dialog = document.querySelector('[data-panel="import"]')
    if (!dialog) return buttonNamed(catalog, 'Import agent')?.click()
    // The dialog binds its fields once it mounts, so the link goes in a beat
    // later. A catalog that mounts again opens a new dialog.
    if (probeImportRead.has(dialog)) return
    probeImportRead.add(dialog)
    setTimeout(() => {
      const current = document.querySelector('[data-panel="import"]')
      const link = current?.querySelector('input.mono')
      if (!link) return
      typeInto(link, 'https://x.ai/bot/rfAHsaFrz6xHBMtUpxDi5')
      setTimeout(() => buttonNamed(current, 'Preview')?.click(), 50)
    }, 100)
    return
  }
  if (agentsState === 'notifications' || agentsState === 'approvals' || agentsState === 'tool-approvals' || agentsState === 'notifications-empty') {
    if (!document.querySelector('[data-panel="notifications"]')) document.querySelector('#sidebar .sidebar-actions button[aria-label^="Notifications"]')?.click()
    return
  }
  if (agentsState.startsWith('usage')) {
    const usage = document.querySelector('[data-panel="usage"]')
    if (!usage) return document.querySelector('#sidebar .sidebar-actions button[aria-label="Usage"]')?.click()
    const pick = (group, label) => { const button = Array.from(usage.querySelectorAll(`[role="radiogroup"][aria-label="${group}"] button`)).find((entry) => entry.textContent.trim() === label); if (button?.getAttribute('aria-checked') === 'false') button.click() }
    if (agentsState === 'usage-tokens' || agentsState === 'usage-partial') pick('Usage view', 'Tokens')
    if (agentsState === 'usage-cost') { pick('Usage view', 'Cost'); pick('Range', '30d') }
    return
  }
  const profile = document.querySelector('aside[aria-label="Agent profile"]')
  if (!profile?.textContent.includes(target.agent)) {
    // An agent opens from its card in Agents, with its chat and its profile.
    const catalog = document.querySelector('[data-panel="agents"]')
    if (!catalog) return probeChatOpened ? undefined : openPage('Agents')
    const card = Array.from(catalog.querySelectorAll('.agent-card')).find((entry) => entry.getAttribute('aria-label') === target.agent)
    // A thread switch can drop the click, so the driver looks again shortly.
    const open = buttonNamed(card, 'Open chat')
    if (open && !open.disabled) { open.click(); probeChatOpened = true }
    setTimeout(advanceAgentsFixture, 400)
    return
  }
  if (agentsState === 'move') {
    const dialog = document.querySelector('[data-panel="move"]')
    if (!dialog) return buttonNamed(profile.querySelector('[aria-label="Primary machine"]'), 'Move')?.click()
    const force = Array.from(dialog.querySelectorAll('label')).find((label) => label.textContent.trim() === 'Force the move')?.querySelector('input')
    if (force && !force.checked) force.click()
    return
  }
  if (agentsState === 'replays') return profile.querySelector('[aria-label="Failed runs"]')?.scrollIntoView({ block: 'start' })
  const editor = document.querySelector('[data-panel="routine"]')
  if (target.routine && !editor) {
    Array.from(profile.querySelectorAll('.routine button')).find((button) => button.textContent.trim() === target.routine)?.click()
  }
  // The webhook editor rotates the secret once, so it shows.
  const rotate = agentsState === 'webhook' && buttonNamed(editor?.querySelector('[aria-label="Webhook address"]'), 'Rotate secret')
  if (rotate && !rotate.disabled) rotate.click()
}

// The permission-modes fixture shows the composer's gate, opens the Modes
// menu, shows a plan gate, or opens Settings → Permissions.
function modesRendered() {
  if (modesState === 'settings') return !!document.querySelector('.permission-settings .group') && !!document.querySelector('.permission-settings [role="switch"]')
  if (modesState === 'new') return !!document.querySelector('[data-panel="mode-picker"] [data-mode="bypass"][aria-checked="true"]')
  const workspace = document.querySelector('.workspace')
  if (!workspace?.querySelector('.permission-card') || !document.querySelector('[data-menu="mode"]')) return false
  if (modesState === 'menu') return !!document.querySelector('[data-panel="mode-picker"]')
  if (modesState === 'plan') return !!workspace.querySelector('.permission-plan')
  return true
}

function advanceModesFixture() {
  if (!modesFixture || fixtureRendered()) return
  if (modesState === 'menu') return document.querySelector('[data-menu="mode"][aria-expanded="false"]')?.click()
  if (modesState === 'new') return document.querySelector('[data-menu="mode"][aria-label="Permission mode: Bypass permissions"][aria-expanded="false"]')?.click()
  if (modesState !== 'settings') return
  if (!document.querySelector('.settings-layout')) return document.querySelector('.sidebar-actions button[aria-label="Settings"][aria-expanded="false"]')?.click()
  const page = Array.from(document.querySelectorAll('#sidebar nav[aria-label="Settings sections"] button')).find((button) => button.textContent.trim() === 'Permissions')
  if (page && page.getAttribute('aria-current') !== 'true') page.click()
}

function fixtureRendered() {
  if (FLEET_PROBE) return fleetRendered()
  if (fleetSyncFixture) return fleetSyncRendered()
  if (agentsFixture) return agentsRendered()
  if (projectsFixture) return projectsRendered()
  if (trustFixture) return trustRendered()
  if (toolsFixture) return toolsRendered()
  if (modesFixture) return modesRendered()
  if (runtimesFixture) return runtimesRendered()
  if (fixtureName === 'runtime-exit') {
    const notice = document.querySelector('[data-testid="runtime-notice"]')
    return notice?.querySelector('p.record')?.textContent === 'The runtime exited.'
      && notice.querySelectorAll('button').length === 1
      && notice.querySelector('button')?.textContent === 'Start runtime'
  }
  if (signedOutFixture) {
    // Signed out is the local workspace with its empty thread and the sidebar's cloud sign-in control.
    const signIn = [...document.querySelectorAll('.workspace button')].find((button) => button.textContent.trim() === 'Sign in to cloud')
    return document.querySelector('.workspace .empty, .workspace .thread-starters') !== null && signIn !== undefined
  }
  if (onboardingFixture) {
    const homePath = document.querySelector('[data-testid="onboarding-home-path"]')
    return homePath?.dataset.homePath === onboardingHomePath
      && document.querySelectorAll('.connect button.option').length >= 9
      && document.querySelector('.connect .action[data-tone="ochre"]')?.textContent.trim() === 'Add key'
  }
  if (scanFixture) {
    // Settings → Data → Memory lists the assistant memory found on this computer.
    const imports = [...document.querySelectorAll('.memory .group')].find((group) => group.querySelector('.settings-heading')?.textContent === 'Imports')
    return imports?.querySelectorAll('.row').length === 2
  }
  if (accessFixture) {
    const sections = document.querySelector('.account-sections')
    const headings = Array.from(sections?.querySelectorAll('.settings-heading') ?? [], (heading) => heading.textContent)
    // The account's devices and phone list on Machines, and retention is on Data.
    return ['Plan', 'Connected programs', 'Your access'].every((heading) => headings.includes(heading))
      && !['Devices', 'Phone pairing', 'Thread retention'].some((heading) => headings.includes(heading))
  }
  const workspace = document.querySelector('.workspace')
  if (!workspace) return false
  // An empty thread shows its one line or, once a model is ready, its starter cards.
  if (history.length === 0) return workspace.querySelector('.empty, .thread-starters') !== null
  if (fixtureName === 'markdown') {
    return workspace.querySelector('.assistant-markdown h3')?.textContent === 'Release notes'
      && workspace.querySelector('.assistant-markdown pre code')
      && workspace.querySelector('.assistant-markdown table')
      && workspace.querySelector('.assistant-markdown a')?.textContent.startsWith('Ready')
  }
  // A long thread renders its newest turns, so the newest reply marks it ready.
  if (fixtureName === 'long-thread') return workspace.querySelector('.thread')?.textContent.includes(history.at(-1).text.split('\n').at(-1))
  const renderedText = workspace.textContent.replaceAll(/\s/g, '')
  return history.every((run) => renderedText.includes((run.prompt ?? '').replaceAll(/\s/g, ''))
    && renderedText.includes(run.text.replaceAll(/\s/g, '')))
}

function advanceScanFixture() {
  if (!scanFixture || fixtureRendered()) return
  if (!document.querySelector('.settings-layout')) {
    document.querySelector('.sidebar-actions button[aria-label="Settings"][aria-expanded="false"]')?.click()
    return
  }
  const memory = Array.from(document.querySelectorAll('#sidebar nav[aria-label="Settings sections"] button'))
    .find((button) => button.textContent.trim() === 'Data')
  if (memory && memory.getAttribute('aria-current') !== 'true') return memory.click()
  Array.from(document.querySelectorAll('.memory button')).find((button) => button.textContent.trim() === 'Look for memory')?.click()
}

// The account sections live in Settings, so the fixture opens that section.
function advanceAccessFixture() {
  if (!accessFixture || fixtureRendered()) return
  const panel = document.querySelector('.settings-layout')
  if (!panel) {
    document.querySelector('.sidebar-actions button[aria-label="Settings"][aria-expanded="false"]')?.click()
    return
  }
  const account = Array.from(document.querySelectorAll('#sidebar nav[aria-label="Settings sections"] button'))
    .find((button) => button.textContent.trim() === 'Account')
  if (account && account.getAttribute('aria-current') !== 'true') account.click()
}

function typeInto(input, value) {
  input.value = value
  input.dispatchEvent(new Event('input', { bubbles: true }))
}

// The Runtimes page lives in Settings, so the fixture opens it, and a pairing
// state pastes a link and presses Pair.
function advanceRuntimesFixture() {
  if (!runtimesFixture || fixtureRendered()) return
  // An open thread keeps its machine, so the picker opens on a new thread,
  // and the remote state opens the thread that lives on Build box.
  if (runtimesState === 'switcher') {
    if (document.querySelector('span.machine-chip')) return document.querySelector('.side-action.new-thread:not(:disabled)')?.click()
    return document.querySelector('button[data-menu="machine"][aria-expanded="false"]')?.click()
  }
  if (runtimesState === 'remote') {
    if (document.querySelector('.thread-title-heading')?.textContent.includes('Nightly test triage')) return
    return Array.from(document.querySelectorAll('#sidebar .thread-row-title')).find((title) => title.textContent.includes('Nightly test triage'))?.closest('button')?.click()
  }
  if (!document.querySelector('.settings-layout')) {
    document.querySelector('.sidebar-actions button[aria-label="Settings"][aria-expanded="false"]')?.click()
    return
  }
  // The scope state stays on Models, the page Settings opens on.
  if (runtimesState === 'scope') return
  const runtimes = Array.from(document.querySelectorAll('#sidebar nav[aria-label="Settings sections"] button'))
    .find((button) => button.textContent.trim() === 'Machines')
  if (runtimes && runtimes.getAttribute('aria-current') !== 'true') return runtimes.click()
  const page = document.querySelector('.settings-runtimes')
  if (!page) return
  // This computer's row opens its network settings, and Add machine its panel.
  if (['pairing-off', 'pairing-on'].includes(runtimesState)) {
    const details = page.querySelector('[aria-label="Settings for This computer"][aria-expanded="false"]')
    if (details) return details.click()
  }
  if (['pairing-off', 'link', 'pairing', 'failed', 'ssh'].includes(runtimesState) && !page.querySelector('.add-machine')) {
    return Array.from(page.querySelectorAll('button')).find((button) => button.textContent.trim() === 'Add machine')?.click()
  }
  if (runtimesState === 'pairing' || runtimesState === 'failed') {
    const link = page.querySelector('input[placeholder="Paste its link"]')
    if (link && !link.value) return typeInto(link, PROBE_PAIRING_LINK)
    const pair = Array.from(page.querySelectorAll('button')).find((button) => button.textContent.trim() === 'Add')
    if (pair && !pair.disabled && !probePairClicked) {
      probePairClicked = true
      pair.click()
    }
    return
  }
  if (runtimesState === 'link') {
    const create = Array.from(page.querySelectorAll('.add-machine button')).find((button) => button.textContent.trim() === 'Show code')
    if (create && !create.disabled && !probePairClicked) {
      probePairClicked = true
      create.click()
    }
    return
  }
  if (runtimesState === 'ssh') {
    const host = page.querySelector('input[placeholder="user@host"]')
    if (!host) {
      return Array.from(page.querySelectorAll('button')).find((button) => button.textContent.trim() === 'Use SSH instead')?.click()
    }
    if (host && !host.value) typeInto(host, 'me@build-box')
  }
}

async function markProbeReady() {
  await document.fonts.ready
  document.body.dataset.probeReady = ''
}

function markReadyAfterFixtureRender() {
  advanceModesFixture()
  advanceScanFixture()
  advanceAccessFixture()
  advanceRuntimesFixture()
  advanceAgentsFixture()
  advanceFleetFixture()
  if (fleetSyncFixture) advanceFleetSync()
  if (projectsFixture) advanceProjects()
  if (trustFixture) advanceTrust()
  if (toolsFixture) advanceTools()
  if (fixtureRendered()) {
    void markProbeReady()
    return
  }
  const observer = new MutationObserver(() => {
    advanceModesFixture()
    advanceScanFixture()
    advanceAccessFixture()
    advanceRuntimesFixture()
    advanceAgentsFixture()
    advanceFleetFixture()
    if (fleetSyncFixture) advanceFleetSync()
    if (projectsFixture) advanceProjects()
    if (trustFixture) advanceTrust()
    if (toolsFixture) advanceTools()
    if (!fixtureRendered()) return
    observer.disconnect()
    void markProbeReady()
  })
  // A fixture can finish on a text change, such as a path or a scan count, or
  // on a control that turns on once its settings load. Agent dialogs mount
  // on the page body, so the agents fixture watches the whole body.
  observer.observe(agentsFixture || fleetSyncFixture || projectsFixture || trustFixture || toolsFixture ? document.body : document.getElementById('app'), { childList: true, subtree: true, characterData: true, attributes: true, attributeFilter: ['disabled', 'aria-checked'] })
}

window.__PROBE__ = {
  eventListeners,
  invokedCommands,
  unknownCommands,
  emit(event, payload) {
    for (const entry of eventListeners.filter((entry) => entry.event === event)) {
      entry.listener({ event, payload })
    }
  },
  // A page that draws itself without the app, such as the avatar gallery, renders here.
  async loadPage(render) {
    await render()
    await markProbeReady()
  },
  async loadBundle() {
    if (fixtureName === 'code-diff' || fixtureName === 'applied-diff') {
      const response = await fetch('/core/protocol-fixtures/code-diff/1/modified.json')
      if (!response.ok) throw new Error(`Could not load the code diff fixture: ${response.status}`)
      const diff = await response.json()
      if (fixtureName === 'code-diff') history[0].pendingPermission.diff = diff
      else {
        runDetails.appliedDiff = diff
        history[0].appliedDiffs[0].summary = diffSummary(diff)
      }
    }
    // `?avatars=colorful|muniment` picks the agent avatar style and `?theme=light|dark`
    // the mode, or a theme id such as `harbor-vibrant-dark` the color scheme, as
    // Appearance would store them, before the app reads them.
    const pick = new URLSearchParams(location.search)
    try {
      if (['colorful', 'muniment'].includes(pick.get('avatars'))) localStorage.setItem('muniment.agent-avatars', pick.get('avatars'))
      if (/^(light|dark|(moss|harbor|clay|iris)(-vibrant|-contrast)?-(light|dark))$/.test(pick.get('theme') ?? '')) localStorage.setItem('muniment.theme', pick.get('theme'))
    } catch (_) {}
    const response = await fetch('/dist/index.html')
    if (!response.ok) throw new Error(`Could not load the built bundle: ${response.status}`)
    const builtPage = new DOMParser().parseFromString(await response.text(), 'text/html')
    for (const stylesheet of builtPage.querySelectorAll('link[rel="stylesheet"]')) {
      const cssResponse = await fetch(`/dist${new URL(stylesheet.href).pathname}`)
      if (!cssResponse.ok) throw new Error(`Could not load the built styles: ${cssResponse.status}`)
      const style = document.createElement('style')
      style.textContent = (await cssResponse.text()).replaceAll('url(/assets/', 'url(/dist/assets/')
      document.head.append(style)
    }
    const module = builtPage.querySelector('script[type="module"][src]')
    if (!module) throw new Error('Could not find the built bundle module.')
    await import(`/dist${new URL(module.src).pathname}`)
    markReadyAfterFixtureRender()
  },
}

window.__TAURI__ = {
  core: {
    async invoke(command, payload) {
      recordInvoke('core', command, payload)
      return probeTable.invoke(command, payload)
    },
  },
  event: {
    async listen(event, listener) {
      const entry = { event, listener }
      eventListeners.push(entry)
      return () => {
        const index = eventListeners.indexOf(entry)
        if (index !== -1) eventListeners.splice(index, 1)
      }
    },
  },
}

// The event plugin's half of an unlisten, for listeners the Tauri API registers.
window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} }

window.__TAURI_INTERNALS__ = {
  metadata: {
    currentWindow: { label: 'main' },
    currentWebview: { label: 'main' },
  },
  async invoke(command, payload) {
    recordInvoke('internal', command, payload)
    if (onboardingFixture && command.includes('open')) return '/Users/alice/Documents/muniment'
    return null
  },
  transformCallback(callback, once = false) {
    const id = callbackId++
    window[`_${id}`] = (value) => {
      if (once) delete window[`_${id}`]
      return callback?.(value)
    }
    return id
  },
  unregisterCallback(id) {
    delete window[`_${id}`]
  },
}
}

// Builds `count` saved facts that mix the three trust levels, short and long
// content, and long titles.
// One add, two updates and one delete over the first three seeded facts,
// newest first. The first update is stale.
function probePending(facts) {
  const [first, second, third] = facts
  const at = (minutes) => new Date(Date.now() - minutes * 60_000).toISOString()
  return [
    { id: 'p1', op: 'add', fact_id: null, before: null, after: { title: 'Standup time', content: 'Joins the team standup at 9:30 each weekday.' }, source: 'thread:t12 run:r31', created: at(4) },
    { id: 'p2', op: 'update', fact_id: first.id, before: { ...first }, after: { title: first.title, content: 'Uses Helix for code and Zed when pairing.' }, source: 'thread:t14 run:r33', created: at(26), stale: true },
    { id: 'p3', op: 'update', fact_id: second.id, before: { ...second }, after: { title: second.title, content: 'Lead with the answer, then the evidence, then one next step.' }, source: 'thread:t14 run:r33', created: at(26) },
    { id: 'p4', op: 'delete', fact_id: third.id, before: { ...third }, after: null, source: 'thread:t15 run:r34', created: at(180) },
  ]
}

function probeFacts(count, prefix = 'f') {
  const subjects = ['Preferred editor', 'Report style', 'Vendor contact', 'Time zone', 'Meeting hours', 'Invoice address', 'Code review habit', 'Travel preference', 'Project codename', 'Reading list', 'Coffee order', 'Release cadence', 'Backup schedule', 'Favorite shell', 'Writing tone', 'Quarterly goals', 'Client naming rule', 'Lease renewal date', 'Test command', 'Deployment window']
  const contents = [
    'Uses Helix for code.',
    'Lead with the answer, then the evidence.',
    'Send invoices to the address on the vendor site.',
    'Works in Eastern time and prefers meetings before noon.',
    'Prefers short replies with one recommendation first, then two alternatives ranked by effort, and a closing step that takes under two minutes to run.',
    'Runs the full test suite before every merge.\nKeeps release notes in the changelog folder.',
    'The lease renews on the first business day of March and needs ninety days of written notice before any change to the terms.',
    'Calls the project Harbor in public documents.',
  ]
  const trusts = ['user', 'conversation', 'conversation', 'external', 'conversation', 'user', 'external']
  return Array.from({ length: count }, (_, index) => {
    const trust = trusts[index % trusts.length]
    const round = Math.floor(index / subjects.length)
    const subject = subjects[index % subjects.length]
    return {
      id: `${prefix}${index + 1}`,
      title: index % 11 === 5 ? `${subject} for the shared research workspace and every client folder` : round ? `${subject} ${round + 1}` : subject,
      content: contents[(index * 3) % contents.length],
      source: trust === 'user' ? 'Added in settings' : `thread:t${index + 1} run:r${index + 1}`,
      trust,
    }
  })
}
