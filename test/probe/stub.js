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


export function buildProbeCommandTable(fixtureName) {
  const fixtureHistory = historyFixtures[fixtureName]
  if (!fixtureHistory) throw new Error(`Unknown probe history fixture: ${fixtureName}`)
  const history = structuredClone(fixtureHistory)
  if (fixtureName === 'applied-diff') history[0].toolActivity = [{ effectId: 'preview-read', displayName: 'read', status: 'completed', input: JSON.stringify({ path: 'src/welcome.js' }) }]
  const onboardingFixture = fixtureName === 'onboarding' || fixtureName === 'scan'
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
  let savedFacts = []
  let deletedFacts = []
  const agentData = { agents: [], state: { runs: {}, threads: {}, primaryThreads: {}, history: {} } }
  const agentMemory = {}
  const projects = { 'project-lease': 'Lease renewal', 'project-research': 'Research' }
  const projectThreads = { 'probe-thread': 'project-lease', 'probe-archive': 'project-lease', 'probe-notes': 'project-research' }
  const eventListeners = []
  const invokedCommands = []
  const unknownCommands = []
  let currentThreadId = history.length ? 'probe-thread' : null
  let retentionChoice = null
  let runtimeState = fixtureName === 'runtime-exit'
    ? { revision: 1, lastEvent: 'exited', visible: true, busy: false }
    : { revision: 0, lastEvent: 'connected', visible: false, busy: false }

  async function invoke(command, payload) {
    if (command === 'launcher_register') return null
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
        { id: 'google', name: 'Google', source: 'key', base_url: null, models: [{ id: 'gemini-3-pro', context: '1M', max_out: '64K', thinking: true, images: true }] },
        { id: 'ollama', name: 'Ollama', source: 'local', base_url: 'http://localhost:11434/v1', models: [{ id: 'llama3.2:3b', context: '128K', max_out: '16.4K', thinking: false, images: false }] },
      ],
      default_provider: 'ollama',
      default_model: 'llama3.2:3b',
      hidden: [],
    }
    if (['local_mode_store_provider_key', 'local_mode_set_default_model', 'local_mode_set_model_hidden', 'local_mode_disconnect_provider', 'local_mode_connect_claude_code', 'local_mode_account_login_start', 'local_mode_account_login_answer', 'local_mode_account_login_cancel', 'local_mode_open_url'].includes(command)) return null
    if (command === 'local_mode_store_endpoint') return 'custom-endpoint'
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
    if (command === 'onboarding_scan') return {
      findings: [
        { assistantId: 'claude-code', displayName: 'Claude Code', root: '/Users/alice/.claude', fileCount: 12, byteTotal: 1024, capped: false, warnings: [] },
        { assistantId: 'pi', displayName: 'Pi', root: '/Users/alice/.pi/agent', fileCount: 3, byteTotal: 512, capped: true, warnings: ['timeCap'] },
      ],
      errors: [],
    }
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
      accounts: [], subscriptions: [], families: [], options: [], routes: [],
      fallback: null, min_confidence: 0.6, served_models: [],
      classifier: { kind: 'none', configured: false, model: '' },
    }
    if (command === 'memory_profile_read') return profileMemory
    if (command === 'memory_profile_save') { profileMemory = payload.content; return null }
    if (command === 'memory_facts') return structuredClone(savedFacts)
    if (command === 'memory_fact_save') {
      const fact = { ...payload.fact, id: payload.fact.id || crypto.randomUUID() }
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
    if (command === 'attach_listener_status') return { started: true, failure: null, connected: false, supervisor_running: false }
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
    const composer = document.querySelector('#first-message')
    const homePath = document.querySelector('[data-testid="onboarding-home-path"]')
    const scan = document.querySelector('[data-testid="onboarding-scan"]')
    return composer && homePath?.textContent === onboardingHomePath
      && document.querySelectorAll('.chips button').length === 3
      && scan?.textContent === 'Claude Code: 12 files · Pi: 3 files'
      && (!scanFixture || document.querySelector('#onboarding-scan-panel li'))
  }
  if (accessFixture) {
    const sections = document.querySelector('.account-sections')
    const headings = Array.from(sections?.querySelectorAll('.access-label') ?? [], (heading) => heading.textContent)
    return ['Thread retention', 'Your access', 'Devices', 'Connected programs', 'Voice shortcut']
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
  document.querySelector('[data-testid="onboarding-scan"][aria-expanded="false"]')?.click()
}

// The account sections live in Settings, so the fixture opens that section.
function advanceAccessFixture() {
  if (!accessFixture || fixtureRendered()) return
  const panel = document.querySelector('.settings-panel')
  if (!panel) {
    document.querySelector('.side-action[aria-haspopup="dialog"][aria-expanded="false"]')?.click()
    return
  }
  const account = Array.from(panel.querySelectorAll('.settings-nav button'))
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
  observer.observe(document.getElementById('app'), { childList: true, subtree: true })
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
