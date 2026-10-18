// The tone each state that core reports reads in, for ui/StatusChip. Core
// decides every state. These only pick its color: success for online and
// done, active for running, danger for a failure, warning for queued and
// waiting, and neutral for offline, idle and anything unknown.

const RUN = {
  queued: 'warning', leased: 'active', running: 'active', waiting_input: 'warning',
  completed: 'success', failed: 'danger', cancelled: 'neutral', interrupted: 'warning',
}
export function runStatusTone(status) {
  return RUN[status] ?? 'neutral'
}

const MACHINE = {
  online: 'success', connecting: 'active', offline: 'neutral', unknown: 'neutral',
  version_mismatch: 'warning', certificate_changed: 'danger', revoked: 'danger', runtime_mismatch: 'danger',
}
export function machineStatusTone(status) {
  return MACHINE[status] ?? 'neutral'
}

// A swarm member reports only whether it answers.
export function onlineTone(online) {
  return online ? 'success' : 'neutral'
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
