import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import { tick } from 'svelte'
import ThinkingPicker from './ThinkingPicker.svelte'
afterEach(cleanup)

it('lists the model levels with the one in use checked and focused', async () => {
  const onchoose = vi.fn()
  render(ThinkingPicker, { levels: ['low', 'medium', 'high', 'xhigh'], current: 'high', onchoose, onclose: vi.fn() })
  const menu = screen.getByRole('menu', { name: 'Thinking level' })
  expect(menu).toHaveAttribute('data-composer-panel')
  const items = screen.getAllByRole('menuitemradio')
  expect(items.map((item) => item.textContent.trim())).toEqual(['Low', 'Medium', 'High', 'Extra high'])
  expect(items.map((item) => item.getAttribute('aria-checked'))).toEqual(['false', 'false', 'true', 'false'])
  expect(items[2].querySelector('[data-icon=check]')).not.toBeNull()
  await tick()
  await tick()
  expect(items[2]).toHaveFocus()
  await fireEvent.click(screen.getByRole('menuitemradio', { name: 'Extra high' }))
  expect(onchoose).toHaveBeenCalledWith('xhigh')
})

it('moves between levels with the arrows and closes on Escape', async () => {
  const onclose = vi.fn()
  render(ThinkingPicker, { levels: ['off', 'low', 'high'], current: 'off', onchoose: vi.fn(), onclose })
  const menu = screen.getByRole('menu', { name: 'Thinking level' })
  const items = screen.getAllByRole('menuitemradio')
  await tick()
  await tick()
  expect(items[0]).toHaveFocus()
  await fireEvent.keyDown(menu, { key: 'ArrowDown' })
  expect(items[1]).toHaveFocus()
  await fireEvent.keyDown(menu, { key: 'ArrowUp' })
  await fireEvent.keyDown(menu, { key: 'ArrowUp' })
  expect(items[2]).toHaveFocus()
  await fireEvent.keyDown(menu, { key: 'Home' })
  expect(items[0]).toHaveFocus()
  await fireEvent.keyDown(menu, { key: 'End' })
  expect(items[2]).toHaveFocus()
  await fireEvent.keyDown(window, { key: 'Escape' })
  expect(onclose).toHaveBeenCalledTimes(1)
})
