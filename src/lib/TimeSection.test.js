import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import TimeSection from './TimeSection.svelte'
import { saveTimeSetting, systemTimeZone, timeSetting } from './time-format.svelte.js'

afterEach(async () => {
  cleanup()
  await saveTimeSetting(null, { zone: '', clock: '' })
})

function runtime() {
  const saves = []
  const invoke = vi.fn(async (command, { operation, body }) => {
    if (operation === 'preferences.set') saves.push(body.values.time)
    return {}
  })
  return { tauri: { invoke }, saves }
}

it('matches this computer until the user picks a clock or a zone', async () => {
  const machine = runtime()
  render(TimeSection, { tauri: machine.tauri })
  const zone = screen.getByRole('radiogroup', { name: 'Time zone' })
  expect(within(zone).getByRole('radio', { name: 'Match this computer' })).toHaveAttribute('aria-checked', 'true')
  expect(screen.getByText(`This computer: ${systemTimeZone()}`)).toBeInTheDocument()
  const clock = screen.getByRole('radiogroup', { name: 'Clock' })
  expect(within(clock).getAllByRole('radio').map((radio) => radio.textContent)).toEqual(['Match this computer', '12-hour', '24-hour'])
  expect(within(clock).getByRole('radio', { name: 'Match this computer' })).toHaveAttribute('aria-checked', 'true')
  expect(screen.queryByRole('listbox', { name: 'Time zones' })).not.toBeInTheDocument()

  await fireEvent.click(within(clock).getByRole('radio', { name: '24-hour' }))
  expect(within(clock).getByRole('radio', { name: '24-hour' })).toHaveAttribute('aria-checked', 'true')
  expect(machine.saves.at(-1)).toMatchObject({ clock: '24h' })
  expect(machine.saves.at(-1)).not.toHaveProperty('zone')
})

it('searches the zone list, saves the pick and returns to this computer', async () => {
  const machine = runtime()
  render(TimeSection, { tauri: machine.tauri })
  await fireEvent.click(screen.getByRole('radio', { name: 'Choose a zone' }))
  // Choosing to pick saves nothing until a zone is picked.
  expect(machine.saves).toEqual([])
  const search = screen.getByRole('searchbox', { name: 'Search time zones' })
  await fireEvent.input(search, { target: { value: 'new york' } })
  const zones = screen.getByRole('listbox', { name: 'Time zones' })
  expect(within(zones).getAllByRole('option').map((option) => option.textContent)).toEqual(['America/New_York'])
  await fireEvent.click(within(zones).getByRole('option', { name: 'America/New_York' }))
  expect(within(zones).getByRole('option', { name: 'America/New_York' })).toHaveAttribute('aria-selected', 'true')
  expect(machine.saves.at(-1)).toMatchObject({ zone: 'America/New_York' })
  expect(timeSetting().zone).toBe('America/New_York')
  await fireEvent.input(search, { target: { value: 'no such place' } })
  expect(screen.getByText('No time zone matches.')).toBeInTheDocument()

  await fireEvent.click(within(screen.getByRole('radiogroup', { name: 'Time zone' })).getByRole('radio', { name: 'Match this computer' }))
  expect(machine.saves.at(-1)).not.toHaveProperty('zone')
  expect(timeSetting().zone).toBe('')
  expect(screen.queryByRole('listbox', { name: 'Time zones' })).not.toBeInTheDocument()
})

it('shows a refused save and keeps the choice before it', async () => {
  const invoke = vi.fn(async () => { throw 'The preferences cannot be saved.' })
  render(TimeSection, { tauri: { invoke } })
  await fireEvent.click(screen.getByRole('radio', { name: '12-hour' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('The preferences cannot be saved.')
  expect(within(screen.getByRole('radiogroup', { name: 'Clock' })).getByRole('radio', { name: 'Match this computer' })).toHaveAttribute('aria-checked', 'true')
})
