// The run-trust probe: two turns that edited a folder through file tools and
// shell commands. `?state=line` opens the last turn's run line,
// `review` opens Review changes on the notes, `rewind` previews a rewind to
// before the second message, `blocked` previews one while another reply
// uses the folder, and `rewound` runs it with one file that fails.
const params = typeof location === 'undefined' ? new URLSearchParams() : new URLSearchParams(location.search)
export const trustState = params.get('state') ?? 'line'

const at = (seconds) => new Date(Date.parse('2026-10-18T12:00:00Z') + seconds * 1000).toISOString()
const tool = (effectId, displayName, input, started, finished, status = 'completed') => ({ effectId, displayName, status, input: JSON.stringify(input), startedAt: at(started), finishedAt: at(finished), textOffset: 0 })

export const trustHistory = [
  {
    runId: 'trust-run-1',
    sentAt: at(0),
    prompt: 'Draft the release notes from the changelog.',
    phase: 'complete',
    text: 'I drafted the release notes in notes.md and rebuilt the docs.',
    receipt: { model: 'anthropic/claude-sonnet-4.5', time: '42.0s', cost: '$0.091 est.', cost_usd: 0.0912, tokens: { input: 15200, output: 3040, total: 18240 }, permission_mode: 'bypass' },
    toolActivity: [
      tool('trust-read', 'read', { path: 'CHANGELOG.md' }, 2, 3),
      tool('trust-search', 'web_search', { query: 'semver release notes format' }, 4, 9),
      tool('trust-edit', 'edit', { path: 'notes.md', oldText: 'Draft\n', newText: 'Release 1.4\nFaster sync\n' }, 10, 11),
      tool('trust-build', 'bash', { command: 'npm run docs' }, 12, 38),
    ],
    resumable: false,
  },
  {
    runId: 'trust-run-2',
    sentAt: at(120),
    prompt: 'Push the notes and tag the release.',
    phase: 'complete',
    text: 'I pushed the notes and tagged v1.4.0.',
    receipt: { model: 'anthropic/claude-sonnet-4.5', time: '18.4s', cost: '$0.034 est.', cost_usd: 0.0341, tokens: { input: 6100, output: 820, total: 6920 }, permission_mode: 'bypass' },
    toolActivity: [
      tool('trust-push', 'bash', { command: 'git push origin main' }, 122, 126),
      tool('trust-tag', 'bash', { command: 'git tag v1.4.0 && git push --tags' }, 127, 130, 'failed'),
      tool('trust-tell', 'agent-message', { to: 'Release bot' }, 131, 132),
    ],
    resumable: false,
  },
]

const changes = (runId, state) => runId === 'trust-run-1' ? {
  run_id: runId,
  folder: '/Users/owner/Documents/muniment/projects/release',
  available: true,
  more: 0,
  files: [
    { path: 'docs/index.html', status: 'modified', additions: 1, deletions: 1, binary: false, state: 'applied', hunks_applied: [true] },
    { path: 'notes.md', status: 'modified', additions: 2, deletions: 1, binary: false, state, hunks_applied: [state === 'applied'] },
  ],
  diff: { schemaVersion: 1, id: `turn-${runId}`, truncated: false, files: [
    { oldPath: 'docs/index.html', newPath: 'docs/index.html', status: 'modified', binary: false, hunks: [{ oldStart: 3, oldCount: 3, newStart: 3, newCount: 3, header: '@@ -3,3 +3,3 @@', lines: [
      { kind: 'context', oldLineNumber: 3, newLineNumber: 3, text: '<main>', segments: [] },
      { kind: 'deletion', oldLineNumber: 4, text: '  <h1>Release 1.3</h1>', segments: [] },
      { kind: 'addition', newLineNumber: 4, text: '  <h1>Release 1.4</h1>', segments: [] },
      { kind: 'context', oldLineNumber: 5, newLineNumber: 5, text: '</main>', segments: [] },
    ] }] },
    { oldPath: 'notes.md', newPath: 'notes.md', status: 'modified', binary: false, hunks: [{ oldStart: 1, oldCount: 1, newStart: 1, newCount: 2, header: '@@ -1,1 +1,2 @@', lines: [
      { kind: 'deletion', oldLineNumber: 1, text: 'Draft', segments: [] },
      { kind: 'addition', newLineNumber: 1, text: 'Release 1.4', segments: [] },
      { kind: 'addition', newLineNumber: 2, text: 'Faster sync', segments: [] },
    ] }] },
  ] },
} : { run_id: runId, folder: '/Users/owner/Documents/muniment/projects/release', available: true, more: 0, files: [], diff: { schemaVersion: 1, id: `turn-${runId}`, files: [], truncated: false } }

let notesState = 'applied'

const preview = (runId) => ({
  thread_id: 'probe-thread',
  run_id: runId,
  available: true,
  turns: 1,
  conversation: 'restore',
  files: {
    checkpoint_id: 'trust-checkpoint',
    folder: '/Users/owner/Documents/muniment/projects/release',
    files_available: true,
    restore: ['CHANGELOG.md', 'assets/logo.png'],
    remove: ['release.tar.gz'],
    cannot_restore: [{ path: 'video/demo.mov', reason: 'too_large' }],
    not_covered: ['.git', 'node_modules'],
    token: 'trust-token',
  },
  irreversible: [
    { kind: 'command', count: 2, labels: ['git push origin main', 'git tag v1.4.0 && git push --tags'] },
    { kind: 'message', count: 1, labels: ['Release bot'] },
  ],
  ...(trustState === 'blocked' ? { blocked: 'Another reply is using this folder. Rewind after it finishes.' } : {}),
})

// Answers the checkpoint operations. A rewind takes its runs out of
// `history`, the stub's thread.
export function trustAnswer(payload, history) {
  const body = payload?.body ?? {}
  if (payload?.operation === 'run.changes') return changes(body.run_id, notesState)
  if (payload?.operation === 'run.revert') {
    notesState = 'reverted'
    return { outcome: { path: body.path, state: 'reverted' }, changes: changes(body.run_id, notesState) }
  }
  if (payload?.operation === 'thread.rewind_preview') return preview(body.run_id)
  if (payload?.operation === 'thread.rewind') {
    const index = history.findIndex((entry) => entry.runId === body.run_id)
    if (index >= 0) history.splice(index)
    return {
      thread_id: body.thread_id, run_id: body.run_id, status: 'partial', turns: 1, conversation: 'restored',
      files: { restored: ['CHANGELOG.md'], removed: ['release.tar.gz'], failed: [{ path: 'assets/logo.png', reason: 'Permission denied.' }], cannot_restore: [{ path: 'video/demo.mov', reason: 'too_large' }], not_covered: ['.git', 'node_modules'], safety_checkpoint: 'trust-safety', complete: false },
    }
  }
  return undefined
}

const named = (root, name) => [...(root?.querySelectorAll('button') ?? [])].find((entry) => entry.getAttribute('aria-label') === name || entry.textContent.trim() === name)
let clicked = new Set()
const once = (key, element) => { if (element && !clicked.has(key)) { clicked.add(key); element.click() } }

// Moves the page to the state's screen, one step per render.
export function advanceTrust() {
  if (trustRendered()) return
  const workspace = document.querySelector('.workspace')
  if (!workspace?.querySelector('[data-testid="run-line"]')) return
  if (trustState === 'line') return once('line', [...workspace.querySelectorAll('details.run-line summary')].at(-1))
  if (trustState === 'review') {
    if (!document.querySelector('.review-panel')) return once('chip', workspace.querySelector('.composer-chips .chip'))
    return once('notes', [...document.querySelectorAll('.review-panel .file')].find((entry) => entry.textContent.includes('notes.md')))
  }
  const rewinds = [...workspace.querySelectorAll('.user-message-meta button[aria-label^="Rewind to here"]')]
  if (!document.querySelector('.rewind-panel') && !document.querySelector('.rewind-notice')) return once('rewind', rewinds[1])
  if (trustState === 'rewound') {
    const go = named(document.querySelector('.rewind-panel'), 'Rewind conversation and files')
    if (go && !go.disabled) once('go', go)
  }
}

export function trustRendered() {
  const workspace = document.querySelector('.workspace')
  if (!workspace) return false
  if (trustState === 'line') return !!workspace.querySelector('details.run-line[open] .timeline li')
  if (trustState === 'review') return !!document.querySelector('.review-panel .hunk tr.addition') && !!document.querySelector('.review-panel .file[aria-current="true"]')?.textContent.includes('notes.md')
  if (trustState === 'rewind') return !!document.querySelector('.rewind-panel #rewind-effects') && !!named(document.querySelector('.rewind-panel'), 'Rewind conversation and files')
  if (trustState === 'blocked') return !!document.querySelector('.rewind-panel [role="alert"]')
  if (trustState === 'rewound') return !!document.querySelector('.rewind-notice details') && !workspace.textContent.includes('I pushed the notes')
  return false
}
