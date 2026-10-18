import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import MachineScope from './MachineScope.svelte'

afterEach(cleanup)

const local = { id: 'local', name: 'This computer', kind: 'local', local: true, status: 'online', active: false }
const container = { id: 'c1', name: 'Muniment runtime', kind: 'container', local: false, status: 'online', active: true }

const shown = () => screen.getByRole('combobox', { name: 'Settings for' }).selectedOptions[0]?.textContent

it('names the selected machine when the machine list and the selection change together', async () => {
  const { rerender } = render(MachineScope, { scope: { selected: local, choices: [local] } })
  expect(shown()).toBe('This computer')
  await rerender({ scope: { selected: container, choices: [local, container] } })
  expect(shown()).toBe('Container')
  // A fresh list from the runtime arrives with new objects for the same machines.
  await rerender({ scope: { selected: { ...container }, choices: [{ ...local }, { ...container }] } })
  expect(shown()).toBe('Container')
})

it('keeps naming the page machine until the switch moves the selection', async () => {
  const onselect = vi.fn()
  render(MachineScope, { scope: { selected: local, choices: [local, container] }, onselect })
  await fireEvent.change(screen.getByRole('combobox', { name: 'Settings for' }), { target: { value: 'c1' } })
  expect(onselect).toHaveBeenCalledWith(container)
  expect(shown()).toBe('This computer')
})
