import { describe, expect, it } from 'vitest'
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

  it('names a schedule in words, and a custom one by its expression', () => {
    expect(cronLabel('0 9 * * *')).toBe('Every day at 09:00')
    expect(cronLabel('0 9 * * 1-5')).toBe('Weekdays at 09:00')
    expect(cronLabel('30 17 * * 5')).toBe('Fridays at 17:30')
    expect(cronLabel('*/15 * * * *')).toBe('Every 15 minutes')
    expect(cronLabel('0 * * * *')).toBe('Every hour')
    expect(cronLabel('0 */2 * * *')).toBe('Every 2 hours')
    expect(cronLabel('0 9 1 * *')).toBe('0 9 1 * *')
  })
})
