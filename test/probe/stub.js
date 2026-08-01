const historyFixtures = {
  'signed-out': [],
  onboarding: [],
  approved: [],
  empty: [],
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
        message: '/Documents/Muniment/exports/old.csv',
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
}

const fixtureName = document.currentScript.dataset.history
const history = historyFixtures[fixtureName]
if (!history) throw new Error(`Unknown probe history fixture: ${fixtureName}`)
const onboardingFixture = fixtureName === 'onboarding' || fixtureName === 'approved'
const approvedFixture = fixtureName === 'approved'
const signedOutFixture = fixtureName === 'signed-out'
const onboardingHomePath = '/Users/alice/Documents/Muniment'
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

const eventListeners = []
const invokedCommands = []
let callbackId = 0
let currentThreadId = history.length ? 'probe-thread' : null

function recordInvoke(surface, command, payload) {
  invokedCommands.push({ surface, command, payload })
}

function fixtureRendered() {
  if (signedOutFixture) {
    const heading = document.querySelector('.lockup h1')
    const signIn = document.querySelector('.auth-state button')
    return heading?.textContent === 'muniment' && signIn?.textContent === 'Sign in'
  }
  if (approvedFixture) {
    const heading = document.querySelector('#onboarding-title')
    const save = document.querySelector('[data-testid="onboarding-import-save"]')
    return heading?.textContent === 'Save approved files' && save?.textContent === 'Save Home and finish'
  }
  if (onboardingFixture) {
    const heading = document.querySelector('#onboarding-title')
    const homePath = document.querySelector('[data-testid="onboarding-home-path"]')
    return heading?.textContent === 'Choose your Muniment Home' && homePath?.textContent === onboardingHomePath
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
  return history.every((run) => renderedText.includes(run.prompt.replaceAll(/\s/g, ''))
    && renderedText.includes(run.text.replaceAll(/\s/g, '')))
}

function advanceApprovedFixture() {
  if (!approvedFixture || fixtureRendered()) return
  const confirm = document.querySelector('[data-testid="onboarding-confirm"]')
  if (confirm) {
    confirm.click()
    return
  }
  const archivePicker = document.querySelector('[data-testid="onboarding-import-picker"]')
  if (archivePicker && !document.querySelector('[aria-label="Export manifest"]')) {
    archivePicker.click()
    return
  }
  const checkbox = document.querySelector('[aria-label="Export manifest"] input[type="checkbox"]')
  if (checkbox && !checkbox.checked) {
    checkbox.click()
    queueMicrotask(advanceApprovedFixture)
    return
  }
  document.querySelector('[data-testid="onboarding-import-continue"]')?.click()
}

async function markProbeReady() {
  await document.fonts.ready
  document.body.dataset.probeReady = ''
}

function markReadyAfterFixtureRender() {
  advanceApprovedFixture()
  if (fixtureRendered()) {
    void markProbeReady()
    return
  }
  const observer = new MutationObserver(() => {
    advanceApprovedFixture()
    if (!fixtureRendered()) return
    observer.disconnect()
    void markProbeReady()
  })
  observer.observe(document.getElementById('app'), { childList: true, subtree: true })
}

window.__PROBE__ = {
  eventListeners,
  invokedCommands,
  emit(event, payload) {
    for (const entry of eventListeners.filter((entry) => entry.event === event)) {
      entry.listener({ event, payload })
    }
  },
  async loadBundle() {
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
      if (command === 'home_status') {
        if (onboardingFixture) return { configured: false, homePath: onboardingHomePath }
        return { configured: true, homePath: '/Documents/Muniment' }
      }
      if (command === 'home_confirm') return { configured: true, homePath: payload.homePath }
      if (command === 'onboarding_import_preview') return {
        entries: [
          { name: 'profile.json', kind: 'json', byteSize: 24, excerpt: '{"name":"Alice"}', excerptTruncated: false },
        ],
        totalByteSize: 24,
      }
      if (command === 'onboarding_import_extract') return [
        { sourceName: 'profile.json', kind: 'json', text: '{"name":"Alice"}', sourceProvenance: 'assistant-export:profile.json' },
      ]
      if (command === 'auth_status') {
        if (signedOutFixture) return { signed_in: false, subject: null }
        return { signed_in: true, subject: 'probe-user' }
      }
      if (command === 'chat_thread_summaries') {
        if (payload.cursor === 'older') return { summaries: structuredClone(olderThreadSummaries), nextCursor: null }
        return { summaries: structuredClone(threadSummaries), nextCursor: history.length ? 'older' : null }
      }
      if (command === 'chat_current_thread') return currentThreadId
      if (command === 'chat_rename_thread') {
        const summary = threadSummaries.find(({ threadId }) => threadId === payload.threadId)
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
        return { runId, attachments: [] }
      }
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
          groups: [],
        }
      }
      if (command === 'auth_devices') return []
      return null
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
    if (approvedFixture && command.includes('open')) return '/Users/alice/Downloads/assistant-export.zip'
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
