// What a failure shows inline and what it keeps behind Show log.

function text(value) {
  if (typeof value === 'string') return value
  if (value && typeof value.message === 'string') return value.message
  return value == null ? '' : String(value)
}

// The first line, ending in a full stop.
export function failureLine(failure, fallback = 'Something went wrong.') {
  const line = text(failure).trim().split('\n')[0]?.trim() || fallback
  return /[.!?]$/.test(line) ? line : `${line}.`
}

// Everything past the first line, and the cause core or the host named, once each.
export function failureLog(failure, detail = '') {
  const full = text(failure).trim()
  const rest = full.split('\n').slice(1).join('\n').trim()
  const cause = text(detail).trim()
  const log = []
  if (rest) log.push(rest)
  if (cause && !full.includes(cause)) log.push(cause)
  return log
}
