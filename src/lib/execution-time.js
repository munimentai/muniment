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

// The milliseconds until `executionTime` shows another second for a run
// `elapsedMs` old: the next half second, where its rounding turns over.
export function nextSecondChange(elapsedMs) {
  if (!Number.isFinite(elapsedMs) || elapsedMs < 0) return 1000
  return 1000 - ((elapsedMs + 500) % 1000) + 1
}

export function receiptTime(value) {
  if (typeof value !== 'string') return value
  const seconds = value.trim().match(/^(\d+(?:\.\d+)?)s$/)
  return seconds ? executionTime(Number(seconds[1])) : value
}
