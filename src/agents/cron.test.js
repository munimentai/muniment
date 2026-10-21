import { afterEach, describe, expect, it } from 'vitest'
import { saveTimeSetting } from '../lib/time-format.svelte.js'
import { INTERVALS, WEEKDAYS, builderFromCron, cronFromBuilder, cronLabel } from './cron.js'

describe('cron builder', () => {
  it('writes each builder choice as the expression it reads back', () => {
    const choices = [
      { mode: 'daily', time: '09:00' },
      { mode: 'daily', time: '23:59' },
      { mode: 'weekdays', time: '07:30' },
      ...WEEKDAYS.map(([weekday]) => ({ mode: 'weekly', time: '17:05', weekday })),
      ...INTERVALS.map((minutes) => ({ mode: 'interval', minutes })),
    ]
    for (const choice of choices) expect(builderFromCron(cronFromBuilder(choice))).toEqual(choice)
  })

  it('writes the expressions the runtime reads', () => {
    expect(cronFromBuilder({ mode: 'daily', time: '09:00' })).toBe('0 9 * * *')
    expect(cronFromBuilder({ mode: 'weekdays', time: '08:15' })).toBe('15 8 * * 1-5')
    expect(cronFromBuilder({ mode: 'weekly', time: '17:30', weekday: '5' })).toBe('30 17 * * 5')
    expect(cronFromBuilder({ mode: 'interval', minutes: 15 })).toBe('*/15 * * * *')
    expect(cronFromBuilder({ mode: 'interval', minutes: 60 })).toBe('0 * * * *')
    expect(cronFromBuilder({ mode: 'interval', minutes: 360 })).toBe('0 */6 * * *')
  })

  it('reads the migrated schedule and Sunday as 7', () => {
    expect(builderFromCron('0 9 * * 1-5')).toEqual({ mode: 'weekdays', time: '09:00' })
    expect(builderFromCron('  0   9 * * 7 ')).toEqual({ mode: 'weekly', time: '09:00', weekday: '0' })
  })

  it('keeps any other expression custom', () => {
    for (const expr of ['', 'not cron', '0 9 1 * *', '0 9 * 1 *', '0 9 * * 1,3', '*/7 * * * *', '0 */5 * * *', '61 9 * * *', '0 24 * * *', '*/15 9 * * *', '0 9 * * * *']) {
      expect(builderFromCron(expr)).toBeNull()
    }
  })

  afterEach(() => saveTimeSetting(null, { zone: '', clock: '' }))

  it('names a schedule in words, and a custom one by its expression', async () => {
    await saveTimeSetting(null, { zone: 'UTC', clock: '24h' })
    expect(cronLabel('0 9 * * *')).toBe('Every day at 09:00')
    expect(cronLabel('0 9 * * 1-5')).toBe('Weekdays at 09:00')
    expect(cronLabel('30 17 * * 5')).toBe('Fridays at 17:30')
    expect(cronLabel('*/15 * * * *')).toBe('Every 15 minutes')
    expect(cronLabel('0 * * * *')).toBe('Every hour')
    expect(cronLabel('0 */2 * * *')).toBe('Every 2 hours')
    expect(cronLabel('0 9 1 * *')).toBe('0 9 1 * *')
    await saveTimeSetting(null, { clock: '12h' })
    expect(cronLabel('0 9 * * 1-5', 'UTC')).toBe('Weekdays at 9:00 AM')
    expect(cronLabel('30 17 * * 5', 'UTC')).toBe('Fridays at 5:30 PM')
  })

  it('reads a schedule in another zone in the zone the Time setting shows, naming the day it falls on', async () => {
    const now = Date.parse('2026-10-20T12:00:00Z')
    await saveTimeSetting(null, { zone: 'Asia/Tokyo', clock: '12h' })
    expect(cronLabel('0 8 * * *', 'Europe/Paris', now)).toBe('Every day at 3:00 PM')
    expect(cronLabel('0 8 * * 1-5', 'Europe/Paris', now)).toBe('Weekdays at 3:00 PM')
    expect(cronLabel('0 20 * * 1-5', 'Europe/Paris', now)).toBe('Tuesdays to Saturdays at 3:00 AM')
    expect(cronLabel('0 20 * * 0', 'Europe/Paris', now)).toBe('Mondays at 3:00 AM')
    await saveTimeSetting(null, { zone: 'America/New_York', clock: '24h' })
    expect(cronLabel('0 1 * * 1', 'Europe/Paris', now)).toBe('Sundays at 19:00')
    expect(cronLabel('0 9 * * *', 'America/New_York', now)).toBe('Every day at 09:00')
  })
})
