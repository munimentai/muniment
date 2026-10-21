import { afterEach, expect, it, vi } from 'vitest'
import { TIME_EVENT, TIME_STORAGE_KEY, dayNumber, displayZone, formatClock, formatDate, formatDateTime, formatHour, formatWallTime, parseTimeSetting, saveTimeSetting, scheduleTime, startTimeSetting, systemClock, systemTimeZone, timeSetting, twelveHour, zoneOffsetMinutes } from './time-format.svelte.js'
import { runTimeLabel } from './agent-labels.js'
import { messageLocalTime } from './chat-state.js'
import { clockTime, readNotifications, scheduleTime as notificationTime } from '../notifications/notifications.js'
import { bucketLabel } from '../usage/usage.js'

afterEach(async () => {
  await saveTimeSetting(null, { zone: '', clock: '' })
  localStorage.clear()
})

const AT = '2026-10-20T20:25:00Z'

it('reads every clock time in the chosen zone on the chosen clock, with no zone name', async () => {
  await saveTimeSetting(null, { zone: 'America/New_York', clock: '12h' })
  expect(formatClock(AT)).toBe('4:25 PM')
  expect(formatHour(AT)).toBe('4 PM')
  expect(formatDateTime(AT)).toBe('Oct 20, 2026, 4:25 PM')
  expect(formatWallTime(8, 0)).toBe('8:00 AM')
  await saveTimeSetting(null, { clock: '24h' })
  expect(formatClock(AT)).toBe('16:25')
  expect(formatHour(AT)).toBe('16')
  expect(formatWallTime(8, 0)).toBe('08:00')
  await saveTimeSetting(null, { zone: 'Asia/Tokyo' })
  expect(formatClock(AT)).toBe('05:25')
  expect(formatDate(AT)).toBe('Oct 21')
  for (const text of [formatClock(AT), formatDateTime(AT)]) expect(text).not.toMatch(/Tokyo|JST|GMT|UTC/)
  expect(formatClock('nonsense')).toBe('')
})

it('matches this computer while nothing is chosen', () => {
  expect(timeSetting()).toEqual({ zone: '', clock: '' })
  expect(displayZone()).toBe(systemTimeZone())
  expect(twelveHour()).toBe(systemClock() === '12h')
  expect(parseTimeSetting('{"zone":"Mars/Base","clock":"13h"}')).toEqual({ zone: '', clock: '' })
  expect(parseTimeSetting({ zone: 'Europe/Paris', clock: '24h', computer_clock: '12h' })).toEqual({ zone: 'Europe/Paris', clock: '24h' })
})

it('counts days and offsets in the chosen zone', async () => {
  await saveTimeSetting(null, { zone: 'Asia/Tokyo', clock: '24h' })
  expect(dayNumber('2026-10-20T16:00:00Z') - dayNumber('2026-10-20T14:00:00Z')).toBe(1)
  expect(zoneOffsetMinutes(new Date(AT))).toBe(540)
  expect(zoneOffsetMinutes(new Date(AT), 'America/New_York')).toBe(-240)
  expect(runTimeLabel('2026-10-20T16:00:00Z', Date.parse('2026-10-20T14:00:00Z'))).toBe('Tomorrow at 01:00')
})

it('reads a schedule in the chosen zone with the day it falls on', async () => {
  const now = Date.parse('2026-10-20T12:00:00Z')
  await saveTimeSetting(null, { zone: 'Europe/Paris', clock: '24h' })
  expect(scheduleTime(8, 0, 'Europe/Paris', now)).toEqual({ time: '08:00', shift: 0 })
  expect(scheduleTime(8, 0, 'America/New_York', now)).toEqual({ time: '14:00', shift: 0 })
  expect(scheduleTime(22, 30, 'America/New_York', now)).toEqual({ time: '04:30', shift: 1 })
  await saveTimeSetting(null, { zone: 'America/New_York', clock: '12h' })
  expect(scheduleTime(1, 0, 'Europe/Paris', now)).toEqual({ time: '7:00 PM', shift: -1 })
  expect(scheduleTime(9, 0, 'Not/AZone', now)).toEqual({ time: '9:00 AM', shift: 0 })
})

it('formats message, notification and usage times through the setting', async () => {
  await saveTimeSetting(null, { zone: 'UTC', clock: '12h' })
  const now = Date.parse('2026-10-20T22:00:00Z')
  expect(messageLocalTime(AT)).toBe('8:25 PM')
  expect(clockTime(AT, now)).toBe('8:25 PM')
  expect(clockTime('2026-10-18T20:25:00Z', now)).toBe('Oct 18')
  expect(notificationTime(AT, now)).toBe('8:25 PM')
  expect(notificationTime('2026-10-21T09:00:00Z', now)).toBe('Wed, Oct 21, 9:00 AM')
  expect(bucketLabel(Date.parse(AT), 'hour')).toBe('8 PM')
  await saveTimeSetting(null, { clock: '24h' })
  expect(messageLocalTime(AT)).toBe('20:25')
  expect(bucketLabel(Date.parse(AT), 'hour')).toBe('20')
  // Notifications group days at the chosen zone's midnight.
  await saveTimeSetting(null, { zone: 'Asia/Kolkata' })
  const invoke = vi.fn(async () => ({}))
  await readNotifications({ invoke }, [])
  expect(invoke.mock.calls[0][1].filter.utc_offset_minutes).toBe(330)
})

it('saves the choice to core on this computer with the computer clock, and keeps a copy', async () => {
  const invoke = vi.fn(async () => ({}))
  const shown = vi.fn()
  window.addEventListener(TIME_EVENT, shown)
  await saveTimeSetting({ invoke }, { zone: 'Asia/Tokyo', clock: '12h' })
  window.removeEventListener(TIME_EVENT, shown)
  expect(invoke).toHaveBeenCalledWith('agent_request', expect.objectContaining({ runtime: 'local', operation: 'preferences.set', body: { values: { time: { zone: 'Asia/Tokyo', clock: '12h', computer_clock: systemClock() } } } }))
  expect(JSON.parse(localStorage.getItem(TIME_STORAGE_KEY))).toEqual({ zone: 'Asia/Tokyo', clock: '12h' })
  expect(shown).toHaveBeenCalledOnce()
  // A refused save restores the choice before it.
  const refuse = vi.fn(async () => { throw 'The preferences cannot be saved.' })
  await expect(saveTimeSetting({ invoke: refuse }, { clock: '24h' })).rejects.toBe('The preferences cannot be saved.')
  expect(timeSetting()).toEqual({ zone: 'Asia/Tokyo', clock: '12h' })
})

it('starts from core and follows each change core announces', async () => {
  let announce
  const invoke = vi.fn(async (command, { operation }) => operation === 'preferences.get' ? { values: { time: { zone: 'Europe/Paris', clock: '24h', computer_clock: systemClock() } } } : {})
  const stop = startTimeSetting({ invoke }, (event, handler) => { announce = handler; return () => {} })
  await vi.waitFor(() => expect(timeSetting()).toEqual({ zone: 'Europe/Paris', clock: '24h' }))
  // Core holds this computer's clock, so nothing is written back.
  expect(invoke.mock.calls.filter(([, args]) => args.operation === 'preferences.set')).toEqual([])
  announce({ payload: { change: { values: { time: { clock: '12h' } } } } })
  expect(timeSetting()).toEqual({ zone: '', clock: '12h' })
  stop()
})

it('tells core this computer clock when core holds none', async () => {
  const invoke = vi.fn(async (command, { operation }) => operation === 'preferences.get' ? { values: {} } : {})
  const stop = startTimeSetting({ invoke }, () => () => {})
  await vi.waitFor(() => expect(invoke.mock.calls.some(([, args]) => args.operation === 'preferences.set')).toBe(true))
  expect(invoke.mock.calls.find(([, args]) => args.operation === 'preferences.set')[1].body).toEqual({ values: { time: { computer_clock: systemClock() } } })
  stop()
})
