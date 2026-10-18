// Run notifications. Core decides which runs warrant one and marks the run
// `notify`. A runtime that does not mark runs yet gets the contract's rule:
// a run that completed, failed or waits for input, which this app did not
// start and which no one started by hand.

const NOTIFY_STATES = new Set(['completed', 'failed', 'waiting_input'])

export function shouldNotify(run, startedHere = new Set()) {
  if (typeof run?.notify === 'boolean') return run.notify
  return NOTIFY_STATES.has(run?.status) && !startedHere.has(run.id) && run.trigger?.kind !== 'manual'
}

// One notification per run and state.
export function notificationKey(run) {
  return `${run.runtimeId ?? 'local'}:${run.id}:${run.status}`
}

export function notificationText(run, agentName) {
  const name = agentName || 'An agent'
  if (run.status === 'failed') return { title: `${name} failed`, body: run.error || 'Open Notifications to see the run.' }
  if (run.status === 'waiting_input') return { title: `${name} is waiting for you`, body: 'Open its conversation to answer.' }
  return { title: `${name} finished`, body: run.silent ? 'Nothing needed a reply.' : run.result_summary || 'Open its conversation to read the result.' }
}

// Shows a system notification when the webview offers one and the user
// allowed it. The Notifications page lists every run either way.
export function showNotification({ title, body }) {
  const Notice = globalThis.Notification
  if (typeof Notice !== 'function') return false
  if (Notice.permission === 'granted') {
    try { new Notice(title, { body }); return true } catch { return false }
  }
  if (Notice.permission === 'default') void Notice.requestPermission?.()
  return false
}
