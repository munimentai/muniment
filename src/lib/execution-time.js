export function executionTime(seconds) {
  if (!Number.isFinite(seconds)) return null
  const total = Math.max(0, Math.round(seconds))
  const hours = Math.floor(total / 3600)
  const minutes = Math.floor(total % 3600 / 60)
  const parts = []
  if (hours) parts.push(`${hours}h`)
  if (hours || minutes) parts.push(`${minutes}m`)
  parts.push(`${total % 60}s`)
  return parts.join(' ')
}

export function receiptTime(value) {
  if (typeof value !== 'string') return value
  const seconds = value.trim().match(/^(\d+(?:\.\d+)?)s$/)
  return seconds ? executionTime(Number(seconds[1])) : value
}
