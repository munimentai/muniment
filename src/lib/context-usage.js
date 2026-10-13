// The words and level of the composer's context meter. The runtime reports
// the counts through `thread_context_usage`; this file only formats them.

// The meter turns to caution at this share of the point where compaction runs.
export const CAUTION_SHARE = 0.85
// The meter shows from this share of the window, and always for an unknown window.
export const SHOW_SHARE = 0.25

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

export function contextMeterShown(usage) {
  if (!usage) return false
  return !usage.windowKnown || contextShare(usage) >= SHOW_SHARE
}

export function contextPercentLabel(usage) {
  if (!usage) return ''
  if (!usage.windowKnown) return '?'
  return `${usage.estimated ? '~' : ''}${Math.round(contextShare(usage) * 100)}%`
}

export function contextHoverLabel(usage) {
  if (!usage || usage.tokens == null) return 'Context usage appears after the next reply'
  if (usage.estimated) return `About ${formatTokens(usage.tokens)} tokens`
  if (!usage.windowKnown || !usage.contextWindow) return `${formatTokens(usage.tokens)} tokens. The model reports no window`
  return `${formatTokens(usage.tokens)} of ${formatTokens(usage.contextWindow)} tokens`
}

// A window string such as `128K` or `1.05M` from Pi's model list, in tokens.
export function windowTokens(text) {
  const match = /^([\d.]+)\s*([KM]?)$/i.exec(String(text ?? '').trim())
  if (!match) return null
  const value = Number(match[1]) * ({ k: 1000, m: 1000000 }[match[2].toLowerCase()] ?? 1)
  return Number.isFinite(value) && value > 0 ? Math.round(value) : null
}

// A window in tokens as the picker shows it: `200K`, `1M`, `1.05M`.
export function windowLabel(tokens) {
  if (!tokens) return ''
  if (tokens >= 1000000) return `${Number((tokens / 1000000).toFixed(2))}M`
  return `${Math.round(tokens / 1000)}K`
}

// Whether the current conversation compacts before its next reply on a model
// with this window, given the reserve the current compaction point implies.
export function compactsOnSwitch(usage, window) {
  if (!usage || usage.tokens == null || !window) return false
  const reserve = usage.contextWindow && usage.compactAt ? usage.contextWindow - usage.compactAt : 16384
  return usage.tokens > window - reserve
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
