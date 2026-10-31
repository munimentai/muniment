import { afterEach, expect, it } from 'vitest'
import { userEvent } from 'vitest/browser'
import { cleanup, render } from '@testing-library/svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import ModelPicker from './ModelPicker.svelte'
afterEach(cleanup)

it('shows search focus once, on the search row, and not as a second ring on the field', async () => {
  const { container } = render(ModelPicker, { inventory: { providers: [{ id: 'openai', name: 'OpenAI', source: 'key', models: [{ id: 'gpt-6.1-sol' }] }] }, onchoose() {}, onmanage() {}, onclose() {} })
  const field = container.querySelector('input[type="search"]')
  const row = container.querySelector('.picker-search')
  const resting = getComputedStyle(row).borderBottomColor
  field.focus()
  await userEvent.keyboard('g')
  expect(document.activeElement).toBe(field)
  expect(field.matches(':focus-visible')).toBe(true)
  expect(getComputedStyle(field).outlineStyle).toBe('none')
  expect(getComputedStyle(row).outlineStyle).toBe('none')
  expect(getComputedStyle(row).borderBottomColor).not.toBe(resting)
})
