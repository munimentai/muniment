const historyFixtures = {
  'signed-out': [],
  onboarding: [],
  approved: [],
  access: [],
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

const fixtureName = document.currentScript.dataset.history
const history = historyFixtures[fixtureName]
if (!history) throw new Error(`Unknown probe history fixture: ${fixtureName}`)
const onboardingFixture = fixtureName === 'onboarding' || fixtureName === 'approved'
const approvedFixture = fixtureName === 'approved'
const accessFixture = fixtureName === 'access'
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
  if (accessFixture) {
    const popover = document.querySelector('.access-popover')
    const headings = Array.from(popover?.querySelectorAll('.access-label') ?? [], (heading) => heading.textContent)
    return ['Appearance', 'Your access', 'Devices', 'Connected programs', 'Voice shortcut']
      .every((heading) => headings.includes(heading))
      && popover.querySelector('.current-device')?.textContent === 'This device'
      && popover.querySelector('.revoked .device-state')?.textContent === 'Revoked'
      && popover.querySelector('.sign-out')?.textContent === 'Sign out'
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

function advanceAccessFixture() {
  if (!accessFixture || fixtureRendered()) return
  document.querySelector('.profile-button[aria-expanded="false"]')?.click()
}

async function markProbeReady() {
  await document.fonts.ready
  document.body.dataset.probeReady = ''
}

function markReadyAfterFixtureRender() {
  advanceApprovedFixture()
  advanceAccessFixture()
  if (fixtureRendered()) {
    void markProbeReady()
    return
  }
  const observer = new MutationObserver(() => {
    advanceApprovedFixture()
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
          groups: [],
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
      if (command === 'attach_listener_status') return { started: true, failure: null, connected: false, supervisor_running: false }
      if (command === 'attach_companions') return [
        {
          identity: '018f0000-0000-7000-8000-000000000001',
          claimed_kind: 'CLI',
          claimed_version: '1.2.3',
          approved_at: '2026-08-04T12:00:00Z',
        },
      ]
      if (command === 'attach_revoke_companion') return null
      throw new Error(`Unknown probe command: ${command}`)
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
