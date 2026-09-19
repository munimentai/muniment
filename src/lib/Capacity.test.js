import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import Capacity from './Capacity.svelte'

afterEach(cleanup)

it('keeps provider allowances separate and shows runtime exclusion evidence', async () => {
  const invoke = vi.fn(async () => ({ enabled: true, running: true, accounts: [
    { id: 'a', label: 'Work', family: 'openai', source: 'account', exclusion_reason: null, windows: [{ label: 'Weekly', remaining_percent: 20 }] },
    { id: 'b', label: 'Personal', family: 'anthropic', source: 'key', exclusion_reason: 'Account is turned off.', windows: [] },
  ] }))
  const { container } = render(Capacity, { tauri: { invoke }, onmanage: vi.fn() })
  const details = container.querySelector('details')
  details.open = true
  await fireEvent(details, new Event('toggle'))
  await screen.findByText('Account is turned off.')
  expect(screen.getByText('Weekly: 20% remaining')).toBeInTheDocument()
  expect(screen.getByText('Remaining allowance unavailable')).toBeInTheDocument()
  expect(screen.getByText('anthropic · Paid API key')).toBeInTheDocument()
  expect(screen.getByText('Ready for supported models')).toBeInTheDocument()
  expect(invoke).toHaveBeenCalledWith('model_router_settings')
})

it('does not claim a ready account when account balancing is disabled', async () => {
  const { container } = render(Capacity, { tauri: { invoke: vi.fn(async () => ({ enabled: false, running: false, accounts: [{ id: 'a', label: 'Work', source: 'key', exclusion_reason: null }] })) }, onmanage: vi.fn() })
  container.querySelector('details').open = true
  await fireEvent(container.querySelector('details'), new Event('toggle'))
  await screen.findByText('Excluded while account balancing is off')
  expect(screen.queryByText('Ready for supported models')).not.toBeInTheDocument()
})
