import { expect, it } from 'vitest'
import { allowanceLabel } from './allowance-label.js'

it('names a window by its period and scope alone', () => {
  expect(allowanceLabel({ label: 'Weekly window', scope: '' })).toBe('Weekly')
  expect(allowanceLabel({ label: '5-hour window' })).toBe('5-hour')
  expect(allowanceLabel({ label: 'Weekly window', scope: 'Fable' })).toBe('Weekly Fable')
  expect(allowanceLabel({ label: 'Window' })).toBe('Allowance')
})
