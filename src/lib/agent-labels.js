// The words the agent card and the agent profile show for a schedule and a run.
const days = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']
export function scheduleLabel(schedule) {
  if (!schedule?.enabled) return ''
  const when = schedule.cadence === 'weekly' ? days[schedule.weekday] : schedule.cadence === 'weekdays' ? 'Weekdays' : 'Every day'
  return `${when} · ${schedule.time}`
}
const runWords = { queued: 'Queued', running: 'Running', waiting: 'Waiting for you', completed: 'Completed', failed: 'Failed', cancelled: 'Stopped', interrupted: 'Interrupted' }
export function runStatusLabel(status) {
  if (!status) return ''
  return runWords[status] || status.charAt(0).toUpperCase() + status.slice(1)
}
