import { expect, it } from 'vitest'
import { PERMISSION_MODES, agentMode, modeIcon, modePermissions } from './permission-modes.js'

it('reads the older permission modes as the modes that replace them', () => {
  expect(agentMode({ permissions: { mode: 'ask' } })?.id).toBe('manual')
  expect(agentMode({ permissions: { mode: 'auto' } })?.id).toBe('auto')
  expect(agentMode({ permissions: { mode: 'strict' } })?.id).toBe('dont_ask')
  expect(agentMode({ permissions: { mode: 'plan' } })?.id).toBe('plan')
  expect(agentMode({})?.id).toBe('manual')
})

it('saves a mode as its older value while one exists, and keeps the rules', () => {
  const agent = { permissions: { mode: 'ask', always_allow: ['rule'], ask_first: [] } }
  const mode = (id) => PERMISSION_MODES.find((entry) => entry.id === id)
  expect(modePermissions(agent, mode('dont_ask'))).toEqual({ mode: 'strict', always_allow: ['rule'], ask_first: [] })
  expect(modePermissions(agent, mode('edit_auto')).mode).toBe('edit_auto')
  // A list core reports saves its ids as they are.
  expect(modePermissions(agent, { id: 'manual' }).mode).toBe('manual')
})

it('names an icon for every mode and keeps bypass off until a machine allows it', () => {
  expect(PERMISSION_MODES.map(modeIcon)).toEqual(['hand', 'pencil', 'map', 'zap', 'shield-check', 'triangle-alert'])
  expect(PERMISSION_MODES.filter((mode) => mode.available === false).map((mode) => mode.id)).toEqual(['bypass'])
})
