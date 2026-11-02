import { expect, it, vi } from 'vitest'
import { focusLikeKeys, trackFocusSource } from './focus-source.js'

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

it('hands the lead to the keys when Enter, Space or Escape presses a control, not when they type', () => {
  const root = document.createElement('div')
  const stop = trackFocusSource(root)
  const button = document.createElement('button')
  const field = document.createElement('input')
  document.body.append(button, field)
  for (const key of ['Enter', ' ', 'Escape']) {
    window.dispatchEvent(new Event('pointerdown'))
    field.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
    expect(root.hasAttribute('data-pointer'), key).toBe(true)
    button.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true }))
    expect(root.hasAttribute('data-pointer'), key).toBe(false)
  }
  stop()
  button.remove(); field.remove()
})

it('focuses a control from code with the ring while the keys lead, and with none while the pointer leads', () => {
  const button = document.createElement('button')
  document.body.append(button)
  const focus = vi.spyOn(button, 'focus')
  expect(focusLikeKeys(button, { preventScroll: true })).toBe(true)
  expect(focus).toHaveBeenLastCalledWith({ preventScroll: true, focusVisible: true })
  expect(document.activeElement).toBe(button)
  button.blur()
  expect(button.hasAttribute('data-key-focus')).toBe(false)
  document.documentElement.dataset.pointer = ''
  try {
    expect(focusLikeKeys(button)).toBe(true)
    expect(focus).toHaveBeenLastCalledWith({ focusVisible: false })
    expect(button.hasAttribute('data-key-focus')).toBe(false)
  } finally { delete document.documentElement.dataset.pointer }
  button.remove()
  expect(focusLikeKeys(button)).toBe(false)
  expect(focusLikeKeys(null)).toBe(false)
})
