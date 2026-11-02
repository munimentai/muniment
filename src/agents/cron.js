// The schedule builder's view of a 5-field cron expression. The runtime
// parses, validates and fires the expression. The builder only writes the
// shapes it offers, and any other expression stays custom.
import { displayZone, scheduleTime } from '../lib/time-format.svelte.js'

export const WEEKDAYS = [['1', 'Monday'], ['2', 'Tuesday'], ['3', 'Wednesday'], ['4', 'Thursday'], ['5', 'Friday'], ['6', 'Saturday'], ['0', 'Sunday']]
export const INTERVALS = [5, 10, 15, 30, 60, 120, 180, 240, 360, 720]

const two = (value) => String(value).padStart(2, '0')

function clock(minute, hour) {
  const m = Number(minute)
  const h = Number(hour)
  if (!Number.isInteger(m) || !Number.isInteger(h) || m < 0 || m > 59 || h < 0 || h > 23) return null
  return `${two(h)}:${two(m)}`
}

// `{ mode, time, weekday, minutes }` for an expression the builder can show,
// or null for one it cannot.
export function builderFromCron(expr) {
  const fields = String(expr ?? '').trim().split(/\s+/)
  if (fields.length !== 5) return null
  const [minute, hour, day, month, weekday] = fields
  if (day !== '*' || month !== '*') return null
  if (/^\d{1,2}$/.test(minute) && /^\d{1,2}$/.test(hour)) {
    const time = clock(minute, hour)
    if (!time) return null
    if (weekday === '*') return { mode: 'daily', time }
    if (weekday === '1-5') return { mode: 'weekdays', time }
    if (/^[0-7]$/.test(weekday)) return { mode: 'weekly', time, weekday: weekday === '7' ? '0' : weekday }
    return null
  }
  if (weekday !== '*') return null
  const everyMinutes = /^\*\/(\d+)$/.exec(minute)
  if (everyMinutes && hour === '*') {
    const minutes = Number(everyMinutes[1])
    return minutes > 0 && minutes < 60 && 60 % minutes === 0 ? { mode: 'interval', minutes } : null
  }
  if (minute === '0' && hour === '*') return { mode: 'interval', minutes: 60 }
  const everyHours = /^\*\/(\d+)$/.exec(hour)
  if (minute === '0' && everyHours) {
    const hours = Number(everyHours[1])
    return hours > 0 && hours < 24 && 24 % hours === 0 ? { mode: 'interval', minutes: hours * 60 } : null
  }
  return null
}

// The expression for a builder choice.
export function cronFromBuilder(choice) {
  if (choice?.mode === 'interval') {
    const minutes = Number(choice.minutes)
    if (minutes < 60) return `*/${minutes} * * * *`
    const hours = minutes / 60
    return hours === 1 ? '0 * * * *' : `0 */${hours} * * *`
  }
  const [hour, minute] = String(choice?.time ?? '09:00').split(':').map(Number)
  const at = `${minute} ${hour}`
  if (choice?.mode === 'weekdays') return `${at} * * 1-5`
  if (choice?.mode === 'weekly') return `${at} * * ${choice.weekday ?? '1'}`
  return `${at} * * *`
}

export function intervalLabel(minutes) {
  if (minutes < 60) return `Every ${minutes} minutes`
  const hours = minutes / 60
  return hours === 1 ? 'Every hour' : `Every ${hours} hours`
}

const dayName = (weekday) => WEEKDAYS.find(([value]) => value === String((Number(weekday) % 7 + 7) % 7))[1]

// The words for a cron trigger in the zone `tz`: the builder's phrase with
// its time read as the Time setting shows times, or the raw expression. A
// time that falls on another day in the shown zone names that day.
export function cronLabel(expr, tz = '', now = Date.now()) {
  const choice = builderFromCron(expr)
  if (!choice) return String(expr ?? '')
  if (choice.mode === 'interval') return intervalLabel(choice.minutes)
  const [hour, minute] = choice.time.split(':').map(Number)
  const { time, shift } = scheduleTime(hour, minute, tz, now)
  if (choice.mode === 'weekdays') return shift ? `${dayName(1 + shift)}s to ${dayName(5 + shift)}s at ${time}` : `Weekdays at ${time}`
  if (choice.mode === 'weekly') return `${dayName(Number(choice.weekday) + shift)}s at ${time}`
  return `Every day at ${time}`
}

// The time zone a new schedule uses: the zone the Time setting shows.
export function localTimeZone() {
  return displayZone()
}
