// The tone each state that core reports reads in, for ui/StatusChip. Core
// decides every state. These only pick its color: success for online and
// done, active for running, danger for a failure or an offline machine,
// warning for queued, waiting and a connecting machine, and neutral for idle
// and anything unknown.

const RUN = {
  queued: 'warning', leased: 'active', running: 'active', waiting_input: 'warning',
  completed: 'success', failed: 'danger', cancelled: 'neutral', interrupted: 'warning',
}
export function runStatusTone(status) {
  return RUN[status] ?? 'neutral'
}

const MACHINE = {
  online: 'success', connecting: 'warning', offline: 'danger', unknown: 'warning',
  version_mismatch: 'warning', certificate_changed: 'danger', revoked: 'danger', runtime_mismatch: 'danger',
}
export function machineStatusTone(status) {
  return MACHINE[status] ?? 'neutral'
}

// A machine's stoplight light for its shape icon: green while it answers,
// yellow while it connects or is not checked yet, red when it does not
// answer or refuses this app, and none for a machine the list does not know.
export function machineLight(runtime) {
  if (!runtime) return null
  if (runtime.local || runtime.status === 'online') return 'green'
  return runtime.status === 'connecting' || runtime.status === 'unknown' || !runtime.status ? 'yellow' : 'red'
}

// A swarm member reports only whether it answers.
export function onlineTone(online) {
  return online ? 'success' : 'danger'
}

const UPDATE = {
  queued: 'warning', requested: 'warning', available: 'warning',
  downloading: 'active', draining: 'active', installing: 'active', restarting: 'active',
  failed: 'danger',
}
// The update state a machine reports, or `requested` and `available` for an
// update this device asked for or the machine offers.
export function updateStatusTone(state) {
  return UPDATE[state] ?? 'neutral'
}
