export const historyFixtures = {
  'signed-out': [],
  'runtime-exit': [],
  onboarding: [],
  scan: [],
  access: [],
  empty: [],
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
        model: 'pi-2',
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
        { effectId: 'probe-unavailable-effect', codeDiffId: 'fixture-unavailable', diff: null },
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
      { ...base, id: 'personal', label: 'personal', source: 'subscription', windows: windows(100) },
      { ...base, id: 'work', label: 'Work', source: 'key', overflow: false, allowance_readable: false, windows: [] },
    ],
  }
}

const providerScanFixtures = {
  mac: {
    servers: [omlx],
    hints: [
      { provider: 'anthropic', evidence: 'Claude Code is signed in' },
      { provider: 'openai', evidence: 'Codex CLI is signed in' },
      { provider: 'github-copilot', evidence: 'Copilot CLI is installed' },
    ],
  },
  many: {
    servers: [
      { id: 'ollama', name: 'Ollama', baseUrl: 'http://127.0.0.1:11434/v1', state: 'ready', modelCount: 3, defaultModel: 'qwen3:8b' },
      omlx,
      { id: 'lmstudio', name: 'LM Studio', baseUrl: 'http://127.0.0.1:1234/v1', state: 'not-running', modelCount: 0 },
    ],
    hints: [{ provider: 'anthropic', evidence: 'Claude Code is signed in' }],
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

export function buildProbeCommandTable(fixtureName) {
  const fixtureHistory = historyFixtures[fixtureName]
  if (!fixtureHistory) throw new Error(`Unknown probe history fixture: ${fixtureName}`)
  const history = structuredClone(fixtureHistory)
  if (fixtureName === 'applied-diff') history[0].toolActivity = [{ effectId: 'preview-read', displayName: 'read', status: 'completed', input: JSON.stringify({ path: 'src/welcome.js' }) }]
  const onboardingFixture = fixtureName === 'onboarding'
  const scanFixture = fixtureName === 'scan'
  const accessFixture = fixtureName === 'access'
  const signedOutFixture = fixtureName === 'signed-out'
  const onboardingHomePath = '/Users/alice/Documents/muniment'
  const threadSummaries = history.length
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
  let recallSettings = null
  // ?memories shows one saved fact of each trust level. ?memories=200 shows
  // that many varied facts and a few deleted ones.
  const memoryCount = Number(new URLSearchParams(location.search).get('memories')) || 0
  let savedFacts = memoryCount ? probeFacts(memoryCount) : new URLSearchParams(location.search).has('memories') ? [
    { id: 'f1', title: 'Preferred editor', content: 'Uses Helix for code.', source: 'Added in settings', trust: 'user' },
    { id: 'f2', title: 'Report style', content: 'Lead with the answer, then the evidence.', source: 'thread:t1 run:r1', trust: 'conversation' },
    { id: 'f3', title: 'Vendor contact', content: 'Send invoices to the address on the vendor site.', source: 'thread:t2 run:r2', trust: 'external' },
  ] : []
  let deletedFacts = memoryCount ? probeFacts(4, 'd').map((fact) => ({ ...fact, title: `Old ${fact.title.toLowerCase()}` })) : []
  const agentData = { agents: [], state: { runs: {}, threads: {}, primaryThreads: {}, history: {} } }
  const agentMemory = {}
  const projects = { 'project-lease': 'Lease renewal', 'project-research': 'Research' }
  const projectThreads = { 'probe-thread': 'project-lease', 'probe-archive': 'project-lease', 'probe-notes': 'project-research' }
  const searchSettings = probeSearch()
  const eventListeners = []
  const invokedCommands = []
  const unknownCommands = []
  let currentThreadId = history.length ? 'probe-thread' : null
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
    if (command === 'artifact_list' || command === 'creation_list') return []
    if (command === 'extend_command' && payload?.action === 'read') return { items: [] }
    if (command === 'browser_view') return null
    if (command === 'runtime_state') return runtimeState
    if (command === 'runtime_start') {
      runtimeState = { revision: 2, lastEvent: 'connected', visible: false, busy: false }
      for (const entry of eventListeners.filter(({ event }) => event === 'runtime-state-changed')) {
        entry.listener({ payload: runtimeState })
      }
      return null
    }
    if (command === 'local_mode_status') return ['local-mode', 'prompt-storage'].includes(fixtureName)
    if (command === 'local_mode_enter') return null
    if (command === 'local_mode_leave') return null
    if (command === 'local_mode_provider_inventory') return {
      providers: [
        ...(new URLSearchParams(location.search).has('accounts') ? [{ id: 'anthropic', name: 'Anthropic', source: 'key', base_url: null, models: [] }] : []),
        { id: 'google', name: 'Google', source: 'key', base_url: null, models: [{ id: 'gemini-3-pro', context: '1M', max_out: '64K', thinking: true, images: true }] },
        { id: 'ollama', name: 'Ollama', source: 'local', base_url: 'http://localhost:11434/v1', models: [{ id: 'llama3.2:3b', context: '128K', max_out: '16.4K', thinking: false, images: false }, { id: 'qwen3:8b', context: '40K', max_out: '8K', thinking: true, images: false }] },
      ],
      default_provider: 'ollama',
      default_model: 'llama3.2:3b',
      hidden: [],
    }
    if (['local_mode_store_provider_key', 'local_mode_set_default_model', 'local_mode_set_model_hidden', 'local_mode_disconnect_provider', 'local_mode_connect_claude_code', 'local_mode_account_login_start', 'local_mode_account_login_answer', 'local_mode_account_login_cancel', 'local_mode_open_url'].includes(command)) return null
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
    if (command === 'record_report') return { report: { open: 0, findings: [], counts: {} } }
    if (command === 'agent_memory') {
      const data = agentMemory[payload.id] ||= { facts: [], deleted: [] }
      if (payload.action === 'memory_facts') return structuredClone(data.facts)
      if (payload.action === 'memory_deleted_facts') return structuredClone(data.deleted)
      if (payload.action === 'memory_fact_save') { const fact = JSON.parse(JSON.stringify({ ...payload.fact, id: payload.fact.id || crypto.randomUUID() })); data.facts = [...data.facts.filter(item => item.id !== fact.id), fact]; return fact }
      if (payload.action === 'memory_fact_delete') { data.deleted.push(...data.facts.filter(item => item.id === payload.factId)); data.facts = data.facts.filter(item => item.id !== payload.factId); return null }
      if (payload.action === 'memory_fact_restore') { data.facts.push(...data.deleted.filter(item => item.id === payload.factId)); data.deleted = data.deleted.filter(item => item.id !== payload.factId); return null }
    }
    if (command === 'agent_import_link') return { url: payload.url, html: await (await fetch('./fixtures/grok-template.html')).text() }
    if (command === 'agent_export_template') return true
    if (command === 'agent_list') return structuredClone(agentData)
    if (command === 'agent_save') {
      const agent = JSON.parse(JSON.stringify({ ...payload.agent, id: payload.agent.id || crypto.randomUUID() }))
      agentData.agents = [...agentData.agents.filter(a => a.id !== agent.id), agent]
      agentData.state.runs[agent.id] ||= { status: 'Ready' }
      return structuredClone(agent)
    }
    if (command === 'agent_delete') { agentData.agents = agentData.agents.filter(a => a.id !== payload.id); delete agentData.state.runs[payload.id]; return null }
    if (command === 'agent_open') return null
    if (command === 'agent_run') { agentData.state.runs[payload.id] = { status: 'queued' }; return null }
    if (command === 'project_list') return { projects: { ...projects }, threads: { ...projectThreads } }
    if (command === 'project_create') {
      if (Object.values(projects).includes(payload.name)) throw new Error('A project with that name already exists.')
      const id = crypto.randomUUID(); projects[id] = payload.name; return id
    }
    if (command === 'project_rename') { projects[payload.projectId] = payload.name; return null }
    if (command === 'project_open') return null
    if (command === 'chat_new_thread') {
      if (payload?.agentId && agentData.state.primaryThreads[payload.agentId]) { currentThreadId = agentData.state.primaryThreads[payload.agentId]; return currentThreadId }
      currentThreadId = crypto.randomUUID()
      const projectId = payload?.projectId || agentData.agents.find(a => a.id === payload?.agentId)?.projectId
      if (projectId) projectThreads[currentThreadId] = projectId
      if (payload?.agentId) { agentData.state.threads[currentThreadId] = payload.agentId; agentData.state.primaryThreads[payload.agentId] = currentThreadId }
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
    if (command === 'chat_thread_open') return { entries: structuredClone(history), nextCursor: null }
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
    history,
    onboardingFixture,
    scanFixture,
    accessFixture,
    signedOutFixture,
    onboardingHomePath,
    threadSummaries,
    olderThreadSummaries,
    eventListeners,
    invokedCommands,
    unknownCommands,
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
  history,
  onboardingFixture,
  scanFixture,
  accessFixture,
  signedOutFixture,
  onboardingHomePath,
  threadSummaries,
  olderThreadSummaries,
  eventListeners,
  invokedCommands,
  unknownCommands,
} = probeTable

function recordInvoke(surface, command, payload) {
  invokedCommands.push({ surface, command, payload })
}

function fixtureRendered() {
  if (fixtureName === 'runtime-exit') {
    const notice = document.querySelector('[data-testid="runtime-notice"]')
    return notice?.querySelector('p.record')?.textContent === 'The runtime exited.'
      && notice.querySelectorAll('button').length === 1
      && notice.querySelector('button')?.textContent === 'Start runtime'
  }
  if (signedOutFixture) {
    // Signed out is the local workspace with its empty thread and the sidebar's cloud sign-in control.
    const signIn = [...document.querySelectorAll('.workspace button')].find((button) => button.textContent.trim() === 'Sign in to cloud')
    return document.querySelector('.workspace .empty') !== null && signIn !== undefined
  }
  if (onboardingFixture) {
    const homePath = document.querySelector('[data-testid="onboarding-home-path"]')
    return homePath?.dataset.homePath === onboardingHomePath
      && document.querySelectorAll('.connect button.option').length >= 9
      && document.querySelector('.connect .action[data-tone="ochre"]')?.textContent.trim() === 'Add key'
  }
  if (scanFixture) {
    // Settings → Memory lists the assistant memory found on this computer.
    const imports = [...document.querySelectorAll('.memory .group')].find((group) => group.querySelector('h4')?.textContent === 'Imports')
    return imports?.querySelectorAll('.row').length === 2
  }
  if (accessFixture) {
    const sections = document.querySelector('.account-sections')
    const headings = Array.from(sections?.querySelectorAll('.settings-heading') ?? [], (heading) => heading.textContent)
    return ['Plan', 'Devices', 'Phone pairing', 'Connected programs', 'Your access', 'Thread retention']
      .every((heading) => headings.includes(heading))
      && sections.querySelector('.current-device')?.textContent === 'This device'
      && sections.querySelector('.revoked .device-state')?.textContent === 'Revoked'
  }
  const workspace = document.querySelector('.workspace')
  if (!workspace) return false
  if (history.length === 0) return workspace.querySelector('.empty') !== null
  if (fixtureName === 'markdown') {
    return workspace.querySelector('.assistant-markdown h3')?.textContent === 'Release notes'
      && workspace.querySelector('.assistant-markdown pre code')
      && workspace.querySelector('.assistant-markdown table')
      && workspace.querySelector('.assistant-markdown a')?.textContent.startsWith('Ready')
  }
  const renderedText = workspace.textContent.replaceAll(/\s/g, '')
  return history.every((run) => renderedText.includes((run.prompt ?? '').replaceAll(/\s/g, ''))
    && renderedText.includes(run.text.replaceAll(/\s/g, '')))
}

function advanceScanFixture() {
  if (!scanFixture || fixtureRendered()) return
  if (!document.querySelector('.settings-layout')) {
    document.querySelector('.side-action[aria-haspopup="dialog"][aria-expanded="false"]')?.click()
    return
  }
  const memory = Array.from(document.querySelectorAll('#sidebar nav[aria-label="Settings sections"] button'))
    .find((button) => button.textContent.trim() === 'Memory')
  if (memory && memory.getAttribute('aria-current') !== 'true') return memory.click()
  Array.from(document.querySelectorAll('.memory button')).find((button) => button.textContent.trim() === 'Look for memory')?.click()
}

// The account sections live in Settings, so the fixture opens that section.
function advanceAccessFixture() {
  if (!accessFixture || fixtureRendered()) return
  const panel = document.querySelector('.settings-layout')
  if (!panel) {
    document.querySelector('.side-action[aria-haspopup="dialog"][aria-expanded="false"]')?.click()
    return
  }
  const account = Array.from(document.querySelectorAll('#sidebar nav[aria-label="Settings sections"] button'))
    .find((button) => button.textContent.trim() === 'Account')
  if (account && account.getAttribute('aria-current') !== 'true') account.click()
}

async function markProbeReady() {
  await document.fonts.ready
  document.body.dataset.probeReady = ''
}

function markReadyAfterFixtureRender() {
  advanceScanFixture()
  advanceAccessFixture()
  if (fixtureRendered()) {
    void markProbeReady()
    return
  }
  const observer = new MutationObserver(() => {
    advanceScanFixture()
    advanceAccessFixture()
    if (!fixtureRendered()) return
    observer.disconnect()
    void markProbeReady()
  })
  // A fixture can finish on a text change, such as a path or a scan count.
  observer.observe(document.getElementById('app'), { childList: true, subtree: true, characterData: true })
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
  async loadBundle() {
    if (fixtureName === 'code-diff' || fixtureName === 'applied-diff') {
      const response = await fetch('/protocol-fixtures/code-diff/1/modified.json')
      if (!response.ok) throw new Error(`Could not load the code diff fixture: ${response.status}`)
      const diff = await response.json()
      if (fixtureName === 'code-diff') history[0].pendingPermission.diff = diff
      else history[0].appliedDiffs[0].diff = diff
    }
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
