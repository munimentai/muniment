// The words and level of the composer's context meter. The runtime reports
// the counts through `thread_context_usage`; this file only formats them.

// The meter turns to caution at this share of the point where compaction runs.
export const CAUTION_SHARE = 0.85

const number = new Intl.NumberFormat('en-US')
export const formatTokens = (value) => number.format(Math.round(value))

// `unknown` when the model reports no window, `caution` near the compaction
// point, `normal` otherwise.
export function contextLevel(usage) {
  if (!usage) return 'normal'
  if (!usage.windowKnown) return 'unknown'
  const limit = usage.compactAt ?? usage.contextWindow
  return usage.tokens != null && limit && usage.tokens >= CAUTION_SHARE * limit ? 'caution' : 'normal'
}

// The share of the window in use, from 0 to 1.
export function contextShare(usage) {
  if (usage?.percent != null) return Math.min(Math.max(usage.percent / 100, 0), 1)
  if (usage?.tokens != null && usage?.contextWindow) return Math.min(usage.tokens / usage.contextWindow, 1)
  return 0
}

export function contextPercentLabel(usage) {
  if (!usage) return ''
  if (!usage.windowKnown) return '?'
  return `${usage.estimated ? '~' : ''}${Math.round(contextShare(usage) * 100)}%`
}

export function contextHoverLabel(usage) {
  if (!usage || usage.tokens == null) return 'Context usage appears after the next reply'
  if (usage.estimated) return `About ${formatTokens(usage.tokens)} tokens. Exact after the next reply`
  if (!usage.windowKnown || !usage.contextWindow) return `${formatTokens(usage.tokens)} tokens. The model reports no window`
  return `${formatTokens(usage.tokens)} of ${formatTokens(usage.contextWindow)} tokens`
}

// The band hint near the compaction point.
export function contextHint(usage) {
  if (contextLevel(usage) !== 'caution') return ''
  return usage.autoCompact ? 'Context is almost full and compacts on its own.' : 'Context is almost full. Compact now to keep replies running.'
}

// `/compact` alone or with text that focuses the summary. Anything else is a message.
export function compactCommand(text) {
  const match = /^\/compact(?:\s+([\s\S]*))?$/.exec(text.trim())
  if (!match) return null
  return { instructions: match[1]?.trim() || null }
}

// The before and after counts that a finished compaction records in its detail.
export function compactionCounts(output) {
  const text = typeof output === 'string' ? output : ''
  const before = /Context before compaction: (\d+) tokens\./.exec(text)
  if (!before) return ''
  const after = /Context after compaction: about (\d+) tokens\./.exec(text)
  return after ? `${formatTokens(Number(before[1]))} → about ${formatTokens(Number(after[1]))} tokens` : `${formatTokens(Number(before[1]))} tokens before`
}
