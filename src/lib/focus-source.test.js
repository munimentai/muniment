import { expect, it } from 'vitest'
import { trackFocusSource } from './focus-source.js'

it('marks the root while the pointer leads and clears it when a key moves focus', () => {
  const root = document.createElement('div')
  const stop = trackFocusSource(root)
  window.dispatchEvent(new Event('pointerdown'))
  expect(root.hasAttribute('data-pointer')).toBe(true)
  // Typing in a field keeps the pointer in the lead.
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'a' }))
  expect(root.hasAttribute('data-pointer')).toBe(true)
  window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab' }))
  expect(root.hasAttribute('data-pointer')).toBe(false)
  stop()
  window.dispatchEvent(new Event('pointerdown'))
  expect(root.hasAttribute('data-pointer')).toBe(false)
})
