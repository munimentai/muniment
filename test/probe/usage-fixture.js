// Usage and Notifications for the probe, in the shapes core's merge answers
// through `usage_summary` and `notifications_list`: three machines, four
// providers, accounts with every meter level, a window that has not started
// and a spent one, pooled windows, providers no window meters with and
// without runs, and runs that run, wait, stop at a limit, fail and finish.
// The `partial` state has the build box give no answer.
import { BOX, STUDIO, probeApprovals } from './agents-fixture.js'

const SCOUT = '018f0000-0000-4000-8000-0000000000a1'
const INBOX = '018f0000-0000-4000-8000-0000000000a2'
const EDITOR = '018f0000-0000-4000-8000-0000000000a3'
const TRIAGE = '018f0000-0000-4000-8000-0000000000a4'
const RECEPTION = '018f0000-0000-4000-8000-0000000000a5'
const HOUR = 3_600_000
const MACHINES = { local: 'This computer', [STUDIO]: 'Studio', [BOX]: 'Build box' }
const ago = (minutes) => new Date(Date.now() - minutes * 60_000).toISOString()

function amount(processed, cost, runs = 1) {
  const cacheRead = Math.round(processed * 0.62)
  const output = Math.round(processed * 0.08)
  const cacheWrite = Math.round(processed * 0.05)
  const input = processed - cacheRead - output - cacheWrite
  return { tokens: { input, output, cache_read: cacheRead, cache_write: cacheWrite, reasoning: 0 }, processed, runs, cost_usd: cost, unpriced_runs: 0 }
}

// A deterministic wave so each bucket differs and the chart reads.
const wave = (index, scale) => Math.round(scale * (0.35 + 0.65 * Math.abs(Math.sin(index * 1.7 + scale % 7))))

function series(range, provider) {
  const hourly = range === '24h'
  const count = { '24h': 24, '7d': 7, '30d': 30, '90d': 90 }[range] ?? 7
  const step = hourly ? HOUR : 24 * HOUR
  const end = new Date()
  if (hourly) end.setMinutes(0, 0, 0)
  else end.setHours(0, 0, 0, 0)
  const scales = { anthropic: hourly ? 90_000 : 1_400_000, openai: hourly ? 60_000 : 900_000, google: hourly ? 20_000 : 300_000, ollama: hourly ? 30_000 : 420_000 }
  const prices = { anthropic: 4.2e-6, openai: 3.1e-6, google: 1.4e-6, ollama: 0 }
  return Array.from({ length: count }, (_, back) => {
    const index = count - 1 - back
    const providers = {}
    for (const [id, scale] of Object.entries(scales)) {
      if (provider && provider !== id) continue
      if ((index + id.length) % 5 === 0 && id === 'google') continue
      const processed = wave(index, scale)
      providers[id] = amount(processed, Number((processed * prices[id]).toFixed(4)), 1 + (index % 3))
    }
    return { start_ms: end.getTime() - back * step, providers }
  }).reverse()
}

function sum(rows) {
  const total = amount(0, 0, 0)
  for (const row of rows) {
    for (const key of Object.keys(total.tokens)) total.tokens[key] += row.tokens[key]
    total.processed += row.processed
    total.runs += row.runs
    total.cost_usd += row.cost_usd
  }
  return total
}

// One card per account with a row per window, as core's `accounts`
// answers, and each provider's pool of a window, as core's `pools` answers
// for the composer.
function windowCard(now, provider, kind, remaining, resetsIn, extra = {}) {
  return {
    provider, kind, scope: '', window_seconds: { five_hour: 18000, daily: 86400, weekly: 604800 }[kind], remaining_percent: remaining,
    ...(resetsIn == null ? { not_started: remaining >= 100 } : { resets_at_ms: now + resetsIn }),
    status: remaining <= 0 ? 'limit' : remaining < 20 ? 'low' : 'ok', ...extra,
  }
}

function accounts(provider, now) {
  const card = (provider, account_id, label, plan, windows, note = null) => ({
    identity: `${provider}:email:${label}:`, provider, account_id, name: { anthropic: 'Anthropic', google: 'Google', openai: 'OpenAI', xai: 'xAI' }[provider],
    subtitle: `${label} · ${plan}`, ...(note ? { note } : {}),
    windows: windows.map(([kind, remaining, resetsIn, scope = '']) => windowCard(now, provider, kind, remaining, resetsIn, { account_id, label, plan, scope })),
  })
  return [
    card('anthropic', 'a-work', 'sam@northwind.example', 'max', [['five_hour', 100, null], ['weekly', 84, 4 * 24 * HOUR + 22 * HOUR], ['weekly', 77, 4 * 24 * HOUR + 22 * HOUR, 'Fable']]),
    card('anthropic', 'a-home', 'sam.home@example.com', 'pro', [['five_hour', 31, HOUR + 40 * 60_000], ['weekly', 12, 2 * 24 * HOUR + 3 * HOUR]]),
    card('google', 'g-1', 'sam@gmail.example', 'ai pro', [['daily', 93, 9 * HOUR]]),
    card('openai', 'o-pro', 'sam@northwind.example', 'pro', [['five_hour', 14, 47 * 60_000], ['weekly', 0, 6 * 24 * HOUR + 20 * HOUR]]),
    card('openai', 'o-home', 'sam.home@example.com', 'plus', [['five_hour', 100, 5 * HOUR], ['weekly', 62, 6 * 24 * HOUR + 7 * HOUR]]),
    card('xai', 'x-1', 'sam@northwind.example', 'SuperGrok', [['weekly', 100, 5 * 24 * HOUR]], 'The plan reports no 5-hour window.'),
  ].filter((account) => !provider || account.provider === provider)
}

function pools(provider, now) {
  return [
    windowCard(now, 'anthropic', 'five_hour', 62, 2 * HOUR + 14 * 60_000),
    windowCard(now, 'anthropic', 'weekly', 81, 4 * 24 * HOUR),
    windowCard(now, 'google', 'daily', 93, 9 * HOUR),
    windowCard(now, 'openai', 'five_hour', 57, 47 * 60_000),
    windowCard(now, 'openai', 'weekly', 0, 6 * 24 * HOUR),
    windowCard(now, 'xai', 'weekly', 100, 5 * 24 * HOUR),
  ].filter((limit) => !provider || limit.provider === provider)
}

// Each provider no window meters with its tokens in the range, which core
// sums from the same model rows.
function unmetered(provider, models, answering) {
  const used = (id) => {
    const rows = models.filter((row) => row.provider === id)
    const input = rows.reduce((sum, row) => sum + row.tokens.input + row.tokens.cache_read + row.tokens.cache_write, 0)
    const output = rows.reduce((sum, row) => sum + row.tokens.output, 0)
    return { input, output, processed: input + output, runs: rows.reduce((sum, row) => sum + row.runs, 0) }
  }
  const held = answering.map((machine) => machine.id)
  return [
    { provider: 'ollama', label: 'Ollama', reason: 'local', endpoint: 'localhost:11434', machines: held.slice(0, 1) },
    { provider: 'kimi', label: 'Kimi', reason: 'pending', machines: held.slice(0, 1) },
    { provider: 'openrouter', label: 'OpenRouter', reason: 'key', machines: held },
  ].filter((entry) => !provider || entry.provider === provider).map((entry) => ({ ...entry, usage: used(entry.provider) }))
}

export function probeUsage({ machines = [], body = {} }, state) {
  const now = Date.now()
  const range = body.range ?? '7d'
  const provider = body.provider ?? null
  const answering = machines.filter((machine) => !(state === 'usage-partial' && machine.id === BOX))
  const share = answering.length / Math.max(1, Object.keys(MACHINES).length)
  const buckets = series(range, provider).map((bucket) => ({ ...bucket, providers: Object.fromEntries(Object.entries(bucket.providers).map(([id, value]) => [id, amount(Math.round(value.processed * share), Number((value.cost_usd * share).toFixed(4)), value.runs)])) }))
  const total = sum(buckets.flatMap((bucket) => Object.values(bucket.providers)))
  const input = total.tokens.input + total.tokens.cache_read + total.tokens.cache_write
  const row = (key, label, part, extra = {}) => ({ key, label, ...amount(Math.round(total.processed * part), Number((total.cost_usd * part).toFixed(4)), Math.max(1, Math.round(total.runs * part))), ...extra })
  const models = [row('anthropic/claude-opus-5', 'claude-opus-5', 0.41, { provider: 'anthropic' }), row('openai/gpt-5.5-codex', 'gpt-5.5-codex', 0.27, { provider: 'openai' }), row('ollama/qwen3:8b', 'qwen3:8b', 0.17, { provider: 'ollama' }), row('anthropic/claude-sonnet-5', 'claude-sonnet-5', 0.09, { provider: 'anthropic' }), row('google/gemini-3-pro', 'gemini-3-pro', 0.06, { provider: 'google' })].filter((entry) => !provider || entry.provider === provider)
  return {
    range, start_ms: buckets[0]?.start_ms, end_ms: now, bucket: range === '24h' ? 'hour' : 'day',
    totals: { processed: total.processed, cached_input: total.tokens.cache_read, uncached_input: total.tokens.input + total.tokens.cache_write, output: total.tokens.output, cache_hit_percent: input ? Math.round(total.tokens.cache_read / input * 1000) / 10 : null, runs: total.runs, cost_usd: total.cost_usd, unpriced_runs: 3 },
    series: buckets,
    breakdown: {
      model: models,
      agent: [row(SCOUT, 'Lead scout', 0.34), row(TRIAGE, 'Nightly test triage', 0.22), row('', '', 0.21), row(INBOX, 'Inbox triage', 0.14), row(EDITOR, 'Release editor', 0.09)],
      project: [row('', '', 0.48), row('p-launch', 'Launch plan', 0.31), row('p-renewals', 'Renewals', 0.21)],
      machine: answering.map((machine, index) => row(machine.id, machine.name, [0.52, 0.31, 0.17][index] ?? 0.1)),
    },
    providers: ['anthropic', 'google', 'ollama', 'openai'],
    accounts: accounts(provider, now),
    pools: pools(provider, now),
    unmetered: unmetered(provider, models, answering),
    windows: [],
    machines: machines.map((machine) => answering.includes(machine) ? { ...machine, state: 'ok' } : { ...machine, state: 'error', error: 'That machine is offline. Try again when it answers.' }),
    partial: answering.length < machines.length,
  }
}

function run(id, agentId, machine, minutesAgo, status, trigger, extra = {}) {
  return {
    status: { completed: 'done', failed: 'failed', running: 'running', waiting_input: 'needs_you', cancelled: 'cancelled' }[status],
    machine: { id: machine, name: MACHINES[machine] ?? 'Office mini' },
    agent_name: extra.name,
    unread: minutesAgo < 300,
    run: { id, agent_id: agentId, thread_id: `thread-${agentId.slice(-2)}`, trigger: { kind: trigger }, status, attempt: 1, hop: 0, created_at: ago(minutesAgo + 3), updated_at: ago(minutesAgo), result_summary: extra.result ?? null, silent: !!extra.silent, error: extra.error ?? null, waiting_since: status === 'waiting_input' ? ago(minutesAgo) : null, reminded_at: null, prompt: '' },
  }
}

// Every approval the swarm holds, as items that need the user.
export function probeApprovalItems() {
  return probeApprovals().map((approval) => ({ kind: 'approval', id: approval.approval_id, machine: { id: 'local', name: 'This computer' }, agent_id: approval.agent_id, agent_name: approval.agent_name, approval, detail: approval.summary, at: approval.created_at }))
}

export function probeNotifications({ machines = [], filter = {} }, state) {
  const now = Date.now()
  const empty = state === 'notifications-empty'
  const scout = { name: 'Lead scout' }, inbox = { name: 'Inbox triage' }, editor = { name: 'Release editor' }, triage = { name: 'Nightly test triage' }, desk = { name: 'Front desk' }
  const entries = empty ? [
    run('n-20', EDITOR, STUDIO, 50, 'completed', 'cron', { ...editor, result: 'Published the weekly changelog draft.' }),
    run('n-21', SCOUT, 'local', 26 * 60, 'completed', 'cron', { ...scout, result: 'Found 4 new leads. Two match the target list.' }),
  ] : [
    run('n-1', SCOUT, 'local', 1, 'running', 'mcp', scout),
    run('n-2', TRIAGE, BOX, 3, 'running', 'cron', triage),
    run('n-3', INBOX, 'local', 6, 'waiting_input', 'cron', { ...inbox, result: 'Two invoices disagree. Which total should I file?' }),
    run('n-4', RECEPTION, 'local', 35, 'failed', 'webhook', { ...desk, error: 'This provider has no remaining allowance. Wait for the reset, add an account, or choose another provider.' }),
    run('n-5', EDITOR, STUDIO, 50, 'completed', 'cron', { ...editor, result: '- **Published the weekly changelog draft.** Two entries cite [the release notes](https://example.com/notes) and `v0.4`.' }),
    run('n-6', SCOUT, 'local', 95, 'completed', 'manual', { ...scout, result: 'Found 4 new leads. Two match the target list.' }),
    run('n-7', TRIAGE, BOX, 140, 'failed', 'webhook', { ...triage, error: 'The test log was not in the webhook payload.' }),
    run('n-8', INBOX, 'local', 200, 'completed', 'cron', { ...inbox, silent: true }),
    run('n-9', EDITOR, STUDIO, 26 * 60, 'completed', 'agent_message', { ...editor, result: 'Answered the beta testers about the v0.4 date.' }),
    run('n-10', SCOUT, 'local', 30 * 60, 'cancelled', 'manual', scout),
    run('n-11', TRIAGE, BOX, 4 * 24 * 60, 'completed', 'cron', { ...triage, result: 'No new failures since Monday.' }),
  ]
  const wanted = (entry) => (!filter.machine || entry.machine.id === filter.machine) && (!filter.agent || entry.run.agent_id === filter.agent)
  const running = entries.filter((entry) => entry.status === 'running' && wanted(entry))
  const rest = entries.filter((entry) => entry.status !== 'running' && wanted(entry))
  const view = filter.view ?? 'all'
  const shown = rest.filter((entry) => view === 'all' || (view === 'failed' && entry.status === 'failed'))
  const dayOf = (entry) => {
    const date = new Date(entry.run.updated_at), today = new Date()
    const yesterday = new Date(today.getTime() - 24 * HOUR)
    return date.toDateString() === today.toDateString() ? 'today' : date.toDateString() === yesterday.toDateString() ? 'yesterday' : 'earlier'
  }
  const days = []
  for (const entry of shown) {
    const day = dayOf(entry)
    if (days.at(-1)?.day === day) days.at(-1).entries.push(entry)
    else days.push({ day, entries: [entry] })
  }
  const needs = empty ? [] : [
    ...probeApprovalItems().slice(0, 3),
    { kind: 'input', id: 'n-3', machine: { id: 'local', name: 'This computer' }, agent_id: INBOX, agent_name: 'Inbox triage', run: entries[2].run, detail: entries[2].run.result_summary, at: entries[2].run.updated_at },
    { kind: 'limit', id: 'n-4', machine: { id: 'local', name: 'This computer' }, agent_id: RECEPTION, agent_name: 'Front desk', run: entries[3].run, resets_at_ms: now + 47 * 60_000, detail: entries[3].run.error, at: entries[3].run.updated_at },
    { kind: 'offline', id: 'office', machine: { id: 'office', name: 'Office mini' }, detail: 'That machine is offline. Try again when it answers.', at: '' },
  ].filter((item) => item.kind === 'offline' ? !filter.agent : (!filter.machine || item.machine.id === filter.machine) && (!filter.agent || item.agent_id === filter.agent))
  const upcoming = [
    { agent_id: SCOUT, agent_name: 'Lead scout', routine_id: 'schedule', routine_name: 'Weekday briefing', at_ms: now + 40 * 60_000, machine: { id: 'local', name: 'This computer' } },
    { agent_id: TRIAGE, agent_name: 'Nightly test triage', routine_id: 'nightly', routine_name: 'Nightly triage', at_ms: now + 9 * HOUR, machine: { id: BOX, name: 'Build box' } },
    { agent_id: EDITOR, agent_name: 'Release editor', routine_id: 'weekly', routine_name: 'Weekly changelog', at_ms: now + 3 * 24 * HOUR, machine: { id: STUDIO, name: 'Studio' } },
  ]
  const names = [...new Map(entries.map((entry) => [entry.run.agent_id, entry.agent_name])).entries()]
  return {
    needs_you: needs,
    running,
    days,
    upcoming,
    counts: { needs_you: needs.filter((item) => item.kind !== 'offline').length, running: running.length, failed: rest.filter((entry) => entry.status === 'failed').length, unread: rest.filter((entry) => entry.unread).length },
    machines: machines.map((machine) => ({ id: machine.id, name: machine.name })),
    agents: names.map(([id, name]) => ({ id, name })),
    read_at: ago(300),
  }
}
