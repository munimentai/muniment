// The Usage page's words and numbers. Core's `usage.summary` sums each
// machine's runs, reads each account's subscription windows and pools them
// by provider, and core joins the machines, so this module only asks,
// formats and names what core answers. Core gives one card per account,
// with a row per window.
import { agentRuntimes } from '../agents/agent-runtime.js'
import { LOCAL_RUNTIME_ID, machineName } from '../runtimes/runtimes.js'
import { familyName, providerName } from '../lib/provider-catalog.js'

export const USAGE_VIEWS = [
  { value: 'limits', label: 'Limits' },
  { value: 'tokens', label: 'Tokens' },
  { value: 'cost', label: 'Cost' },
]
export const USAGE_RANGES = [
  { value: '24h', label: '24h' },
  { value: '7d', label: '7d' },
  { value: '30d', label: '30d' },
  { value: '90d', label: '90d' },
]
export const BREAKDOWNS = [
  { value: 'model', label: 'Model' },
  { value: 'agent', label: 'Agent' },
  { value: 'project', label: 'Project' },
  { value: 'machine', label: 'Machine' },
]

// This computer and each paired machine that answers, each under the one
// name the Machines list shows for it.
export function machineRefs(runtimeList) {
  return agentRuntimes(runtimeList).map((runtime) => ({
    id: runtime.local ? LOCAL_RUNTIME_ID : runtime.id,
    name: machineName(runtime),
  }))
}

// One request through the window: each named machine answers and core joins them.
export function readUsage(tauri, machines, { range = '7d', provider = null } = {}) {
  const body = { range, utc_offset_minutes: -new Date().getTimezoneOffset() }
  if (provider) body.provider = provider
  return tauri.invoke('usage_summary', { machines, body })
}

// Each provider's bar color: its brand's own hue, a neutral for the rest.
const PROVIDER_COLORS = {
  anthropic: '#D97757',
  openai: '#10A37F',
  google: '#4285F4',
  xai: '#8E9196',
  kimi: '#2BB3C0',
  meta: '#C2185B',
  devin: '#7A8B3A',
}
const SPARE_COLORS = ['#B07A3A', '#5E8C6A', '#8C6A5E', '#4F7A9A', '#9A7F4F']
export function providerColor(provider) {
  if (PROVIDER_COLORS[provider]) return PROVIDER_COLORS[provider]
  const hash = [...String(provider ?? '')].reduce((sum, char) => sum + char.charCodeAt(0), 0)
  return SPARE_COLORS[hash % SPARE_COLORS.length]
}

// A family's name, else the catalog's name for a provider such as a local server.
export function providerLabel(provider) {
  if (!provider) return 'Unknown'
  const family = familyName(provider)
  return family !== provider ? family : providerName(provider) || provider
}

const WINDOW_NAMES = { five_hour: '5-hour', daily: 'Daily', weekly: 'Weekly', monthly: 'Monthly', other: 'Window' }
export function windowLabel(limit) {
  const name = WINDOW_NAMES[limit?.kind] ?? 'Window'
  return limit?.scope ? `${name} ${limit.scope}` : name
}

// "Resets in 2h 14m", or the date for a reset more than a day away.
export function resetsIn(at, now = Date.now()) {
  if (!Number.isFinite(at)) return ''
  const left = Math.max(0, at - now)
  const minutes = Math.round(left / 60_000)
  if (minutes < 1) return 'Resets now'
  if (minutes < 60) return `Resets in ${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `Resets in ${hours}h ${minutes % 60}m`
  const days = Math.floor(hours / 24)
  return `Resets in ${days}d ${hours % 24}h`
}

export function percentLeft(value) {
  return `${Math.round(Number(value) || 0)}% left`
}

// A window's line under its name: the reset, "Not started" for a window
// core says starts on the next use, else that the upstream named no reset.
export function resetLine(limit, now = Date.now()) {
  if (limit?.not_started) return 'Not started'
  return resetsIn(limit?.resets_at_ms, now) || 'Reset time unknown'
}

// The meter's lit steps by share left: 4 from 75%, 3 from 50%, 2 from 25%,
// 1 under that, and none at the limit.
export function meterLevel(limit) {
  const left = Number(limit?.remaining_percent) || 0
  if (limit?.status === 'limit' || left <= 0) return 0
  return left >= 75 ? 4 : left >= 50 ? 3 : left >= 25 ? 2 : 1
}

const RANGE_PHRASES = { '24h': 'the last 24 hours', '7d': 'the last 7 days', '30d': 'the last 30 days', '90d': 'the last 90 days' }
export function rangePhrase(range) {
  return RANGE_PHRASES[range] ?? RANGE_PHRASES['7d']
}

// 950, 12.4K, 3.1M, 2.0B.
export function compactTokens(count) {
  const value = Number(count) || 0
  if (value < 1000) return String(Math.round(value))
  const units = [['B', 1e9], ['M', 1e6], ['K', 1e3]]
  const [unit, size] = units.find(([, size]) => value >= size)
  const scaled = value / size
  return `${scaled >= 100 ? Math.round(scaled) : scaled.toFixed(1)}${unit}`
}

export function dollars(amount) {
  const value = Number(amount) || 0
  if (value > 0 && value < 0.01) return '<$0.01'
  return `$${value.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`
}

// The five numbers above the chart.
export function totalTiles(totals, view) {
  const t = totals ?? {}
  if (view === 'cost') return [
    { label: 'API estimate', value: dollars(t.cost_usd) },
    { label: 'Runs', value: String(t.runs ?? 0) },
    { label: 'Per run', value: t.runs ? dollars((t.cost_usd ?? 0) / t.runs) : '$0.00' },
    { label: 'Cache hit rate', value: t.cache_hit_percent == null ? 'None' : `${t.cache_hit_percent}%` },
    { label: 'Unpriced runs', value: String(t.unpriced_runs ?? 0) },
  ]
  return [
    { label: 'Processed', value: compactTokens(t.processed) },
    { label: 'Cached input', value: compactTokens(t.cached_input) },
    { label: 'Uncached input', value: compactTokens(t.uncached_input) },
    { label: 'Output', value: compactTokens(t.output) },
    { label: 'Cache hit rate', value: t.cache_hit_percent == null ? 'None' : `${t.cache_hit_percent}%` },
  ]
}

// The amount one view measures: processed tokens or estimated dollars.
export function measure(amount, view) {
  return view === 'cost' ? Number(amount?.cost_usd) || 0 : Number(amount?.processed) || 0
}

export function measureLabel(value, view) {
  return view === 'cost' ? dollars(value) : compactTokens(value)
}

// A bucket's short axis label: the hour for a day, else the day of the month.
export function bucketLabel(startMs, bucket) {
  const at = new Date(startMs)
  if (bucket === 'hour') return at.toLocaleTimeString([], { hour: 'numeric' })
  return at.toLocaleDateString([], { month: 'short', day: 'numeric' })
}

// A breakdown row's name: the label core gives, else what an empty key means.
export function rowLabel(row, breakdown) {
  if (row?.label) return row.label
  if (row?.key) return row.key
  return breakdown === 'agent' ? 'No agent' : breakdown === 'project' ? 'No project' : 'Unknown'
}

// Every provider the page can filter by: those with usage and those with a window.
export function providerOptions(summary) {
  const ids = new Set([...(summary?.providers ?? []), ...(summary?.accounts ?? []).map((account) => account.provider)])
  return [{ value: '', label: 'All providers' }, ...[...ids].sort().map((id) => ({ value: id, label: providerLabel(id), provider: id }))]
}

const UNMETERED = { local: 'No limit', key: 'Billed by use', pending: 'Allowance pending', unread: 'Allowance unavailable' }
export function unmeteredLabel(entry) {
  return UNMETERED[entry?.reason] ?? 'No limit'
}

// A local server's address and the machine it runs on, such as
// "127.0.0.1:8080 · on this computer". `machines` is the page's machine list.
export function unmeteredSubtitle(entry, machines = []) {
  const held = entry?.machines ?? []
  const named = machines.find((machine) => machine.id === held[0])?.name
  const where = entry?.reason !== 'local' || !held.length ? ''
    : held.length > 1 ? `on ${held.length} machines`
    : held[0] === LOCAL_RUNTIME_ID ? 'on this computer'
    : named ? `on ${named}` : ''
  return [entry?.endpoint, where].filter(Boolean).join(' · ')
}

// The pool the composer warns about: a spent window first, else one under a
// tenth left, for the provider in use when one is known. Core's `pools`
// spend a provider's window only once every account in it is spent.
export function composerLimit(limits, provider = null) {
  const pool = (limits ?? []).filter((limit) => !provider || limit.provider === provider)
  return pool.find((limit) => limit.status === 'limit')
    ?? pool.filter((limit) => limit.status === 'low' && limit.remaining_percent < 10).sort((a, b) => a.remaining_percent - b.remaining_percent)[0]
    ?? null
}

// Settings opens the Usage page through the shell, which owns the pages.
export const OPEN_USAGE_EVENT = 'muniment:open-usage'
export function openUsagePage() {
  try { window.dispatchEvent(new CustomEvent(OPEN_USAGE_EVENT)) } catch (_) {}
}

export function isUsageShortcut(event, platform = navigator.platform) {
  const mac = platform.startsWith('Mac')
  return event.key?.toLowerCase() === 'u'
    && (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey)
    && !event.altKey
    && !!event.shiftKey
}

export function usageShortcut(platform = navigator.platform) {
  return platform.startsWith('Mac') ? 'Meta+Shift+U' : 'Control+Shift+U'
}
