import { expect, it } from 'vitest'
import { runStatusLabel, scheduleLabel } from './agent-labels.js'
it('names a schedule by its days and time, and no schedule by nothing', () => {
  expect(scheduleLabel({ enabled: true, cadence: 'weekdays', time: '08:00' })).toBe('Weekdays · 08:00')
  expect(scheduleLabel({ enabled: true, cadence: 'weekly', weekday: 4, time: '17:30' })).toBe('Friday · 17:30')
  expect(scheduleLabel({ enabled: true, cadence: 'daily', time: '09:00' })).toBe('Every day · 09:00')
  expect(scheduleLabel({ enabled: false, cadence: 'daily', time: '09:00' })).toBe('')
  expect(scheduleLabel(null)).toBe('')
})
it('shows run states in sentence case, including ones it does not know', () => {
  expect(runStatusLabel('completed')).toBe('Completed')
  expect(runStatusLabel('waiting')).toBe('Waiting for you')
  expect(runStatusLabel('paused')).toBe('Paused')
  expect(runStatusLabel('')).toBe('')
})
