// Every clock time the window shows reads through this module: in this
// computer's time zone or one the user picks, on the clock of the computer's
// locale or a 12- or 24-hour clock the user picks. Core keeps the choice as
// the machine-local `time` preference and phrases its own times with it.
// localStorage keeps a copy for the first paint and stays the only store
// when the runtime refuses the operations. Times name no time zone, and
// relative times such as `2h` do not change. The state is shared, so a view
// that formats a time redraws when the choice changes.
import { agentRequest } from '../agents/agent-runtime.js'
import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'

export const TIME_KEY = 'time'
export const TIME_STORAGE_KEY = 'muniment.time'
export const TIME_EVENT = 'muniment:time'
export const CLOCK_OPTIONS = [['', 'Match this computer'], ['12h', '12-hour'], ['24h', '24-hour']]

// A stored choice: `zone` an IANA zone and `clock` `12h` or `24h`, each
// empty to match this computer.
export function parseTimeSetting(value) {
  let stored = value
  if (typeof value === 'string') { try { stored = JSON.parse(value) } catch (_) { stored = null } }
  const zone = typeof stored?.zone === 'string' && validZone(stored.zone) ? stored.zone : ''
  const clock = stored?.clock === '12h' || stored?.clock === '24h' ? stored.clock : ''
  return { zone, clock }
}

function validZone(zone) {
  try { new Intl.DateTimeFormat('en-US', { timeZone: zone }); return true } catch (_) { return false }
}

function readStored() {
  try { return parseTimeSetting(localStorage.getItem(TIME_STORAGE_KEY)) } catch (_) { return parseTimeSetting(null) }
}

const setting = $state(readStored())

export function timeSetting() {
  return { zone: setting.zone, clock: setting.clock }
}

// This computer's zone and the clock of its locale.
export function systemTimeZone() {
  try { return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC' } catch (_) { return 'UTC' }
}

export function systemClock() {
  try {
    const cycle = new Intl.DateTimeFormat(undefined, { hour: 'numeric' }).resolvedOptions().hourCycle
    return cycle === 'h11' || cycle === 'h12' ? '12h' : '24h'
  } catch (_) {
    return '12h'
  }
}

// Every IANA zone the list offers.
export function timeZones() {
  try { return Intl.supportedValuesOf('timeZone') } catch (_) { return [] }
}

export function displayZone() {
  return setting.zone || systemTimeZone()
}

export function twelveHour() {
  return (setting.clock || systemClock()) === '12h'
}

const valid = (value) => {
  const date = value instanceof Date ? value : new Date(value)
  return Number.isFinite(date.getTime()) ? date : null
}

const clockOptions = () => twelveHour() ? { hour: 'numeric', minute: '2-digit', hour12: true } : { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }

// The clock time of an instant: `4:25 PM` or `16:25`.
export function formatClock(value) {
  const date = valid(value)
  return date ? date.toLocaleTimeString('en-US', { ...clockOptions(), timeZone: displayZone() }) : ''
}

// The hour alone, for a chart axis: `4 PM` or `16`.
export function formatHour(value) {
  const date = valid(value)
  if (!date) return ''
  return date.toLocaleTimeString('en-US', twelveHour() ? { hour: 'numeric', hour12: true, timeZone: displayZone() } : { hour: '2-digit', hourCycle: 'h23', timeZone: displayZone() })
}

// The date of an instant in the shown zone, with `options` such as
// `{ month: 'short', day: 'numeric' }`.
export function formatDate(value, options = { month: 'short', day: 'numeric' }) {
  const date = valid(value)
  return date ? date.toLocaleDateString('en-US', { ...options, timeZone: displayZone() }) : ''
}

// The date and the clock time: `Oct 20, 2026, 4:25 PM`.
export function formatDateTime(value) {
  const date = valid(value)
  return date ? `${formatDate(date, { month: 'short', day: 'numeric', year: 'numeric' })}, ${formatClock(date)}` : ''
}

// An hour and minute of the day as the clock shows it, with no zone change.
export function formatWallTime(hour, minute) {
  return new Date(Date.UTC(2000, 0, 1, hour, minute)).toLocaleTimeString('en-US', { ...clockOptions(), timeZone: 'UTC' })
}

// The calendar day of an instant in the shown zone, as a day count, so two
// instants compare by day.
export function dayNumber(value) {
  const date = valid(value)
  if (!date) return NaN
  const parts = Object.fromEntries(new Intl.DateTimeFormat('en-US', { timeZone: displayZone(), year: 'numeric', month: 'numeric', day: 'numeric' }).formatToParts(date).map(({ type, value: part }) => [type, Number(part)]))
  return Math.round(Date.UTC(parts.year, parts.month - 1, parts.day) / 86_400_000)
}

export function sameDay(a, b) {
  return dayNumber(a) === dayNumber(b)
}

export function sameYear(a, b) {
  return formatDate(a, { year: 'numeric' }) === formatDate(b, { year: 'numeric' })
}

// How many minutes the zone `zone` runs ahead of UTC at an instant.
export function zoneOffsetMinutes(at = new Date(), zone = displayZone()) {
  const date = valid(at) ?? new Date()
  try {
    const parts = Object.fromEntries(new Intl.DateTimeFormat('en-US', { timeZone: zone, hourCycle: 'h23', year: 'numeric', month: 'numeric', day: 'numeric', hour: 'numeric', minute: 'numeric' }).formatToParts(date).map(({ type, value }) => [type, Number(value)]))
    const wall = Date.UTC(parts.year, parts.month - 1, parts.day, parts.hour % 24, parts.minute)
    return Math.round((wall - Math.floor(date.getTime() / 60_000) * 60_000) / 60_000)
  } catch (_) {
    return -date.getTimezoneOffset()
  }
}

// A schedule's hour and minute in the zone `tz`, read in the shown zone on
// the schedule zone's date at `now`: the clock time and how many days the
// shown date runs ahead, from -1 to 1. Core's `time_display` reads the same.
export function scheduleTime(hour, minute, tz, now = Date.now()) {
  const zone = displayZone()
  if (!tz || tz === zone || !validZone(tz)) return { time: formatWallTime(hour, minute), shift: 0 }
  const today = new Date(now)
  const parts = Object.fromEntries(new Intl.DateTimeFormat('en-US', { timeZone: tz, year: 'numeric', month: 'numeric', day: 'numeric' }).formatToParts(today).map(({ type, value }) => [type, Number(value)]))
  const wall = Date.UTC(parts.year, parts.month - 1, parts.day, hour, minute)
  // The instant is the wall time less the zone's offset at that instant.
  let at = wall - zoneOffsetMinutes(wall, tz) * 60_000
  at = wall - zoneOffsetMinutes(at, tz) * 60_000
  const shift = Math.max(-1, Math.min(1, dayNumber(at) - Math.floor(wall / 86_400_000)))
  return { time: formatClock(at), shift }
}

function apply(value) {
  const next = parseTimeSetting(value)
  setting.zone = next.zone
  setting.clock = next.clock
  try { localStorage.setItem(TIME_STORAGE_KEY, JSON.stringify(next)) } catch (_) {}
  try { window.dispatchEvent(new CustomEvent(TIME_EVENT, { detail: next })) } catch (_) {}
  return next
}

// The value core stores: the choice and the clock this computer's locale
// uses, which core cannot read itself.
const stored = (next) => ({ ...(next.zone ? { zone: next.zone } : {}), ...(next.clock ? { clock: next.clock } : {}), computer_clock: systemClock() })

// Saves a choice to core on this computer and shows it at once. A runtime
// that refuses keeps it in localStorage alone.
export async function saveTimeSetting(tauri, change) {
  const previous = timeSetting()
  const next = apply({ ...previous, ...change })
  if (!tauri) return next
  try {
    await agentRequest(tauri, LOCAL_RUNTIME_ID, 'preferences.set', { values: { [TIME_KEY]: stored(next) } })
  } catch (error) {
    if (!/not a preference|unknown/i.test(String(error?.message ?? error))) {
      apply(previous)
      throw error
    }
  }
  return next
}

// Reads core's choice, tells core this computer's clock when it changed,
// and follows each change core announces. Answers the function that stops it.
export function startTimeSetting(tauri, listen) {
  if (!tauri) return () => {}
  let stopped = false
  const stops = []
  void Promise.resolve(listen?.('preferences-changed', ({ payload }) => {
    const values = payload?.change?.values
    if ((payload?.runtimeId || LOCAL_RUNTIME_ID) === LOCAL_RUNTIME_ID && values && TIME_KEY in values) apply(values[TIME_KEY])
  })).then((unlisten) => { if (stopped) unlisten?.(); else stops.push(unlisten) }).catch(() => {})
  void (async () => {
    const answer = await agentRequest(tauri, LOCAL_RUNTIME_ID, 'preferences.get', {})
    if (stopped) return
    const held = answer?.values?.[TIME_KEY]
    const next = held ? apply(held) : timeSetting()
    if (held?.computer_clock !== systemClock()) {
      await agentRequest(tauri, LOCAL_RUNTIME_ID, 'preferences.set', { values: { [TIME_KEY]: stored(next) } })
    }
  })().catch(() => {})
  return () => {
    stopped = true
    for (const stop of stops) stop?.()
  }
}
