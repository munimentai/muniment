export function relativeTime(timestamp, now = new Date()) {
  const date = new Date(timestamp)
  if (Number.isNaN(date.getTime())) return ''
  const current = new Date(now)
  if (date.getFullYear() !== current.getFullYear()) {
    return date.toLocaleDateString('en-US', { day: 'numeric', month: 'short', year: 'numeric' })
  }
  const elapsed = Math.max(0, current.getTime() - date.getTime())

  if (elapsed < 60_000) return 'Just now'
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)}m`
  if (elapsed < 86_400_000) return `${Math.floor(elapsed / 3_600_000)}h`
  if (elapsed < 604_800_000) return `${Math.floor(elapsed / 86_400_000)}d`
  return date.toLocaleDateString('en-US', { day: 'numeric', month: 'short' })
}
