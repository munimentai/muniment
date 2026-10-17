import fs from 'node:fs'
import path from 'node:path'
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import PermissionSettings from './PermissionSettings.svelte'
afterEach(cleanup)

const fixture = (name) => JSON.parse(fs.readFileSync(path.join(process.cwd(), 'core/protocol-fixtures/muniment.attach/1', name), 'utf8')).body

// Answers the permission operations from one stored settings file, as core does.
function runtime(refuse = null) {
  let settings = structuredClone(fixture('response-permission-settings.json'))
  const requests = []
  const invoke = vi.fn(async (command, args) => {
    requests.push(args)
    if (args.operation === 'permission.settings') return structuredClone(settings)
    if (args.operation === 'permission.modes') {
      const modes = structuredClone(fixture('response-permission-modes.json'))
      return { ...modes, modes: modes.modes.map((mode) => mode.id === 'bypass' ? { ...mode, available: settings.bypass_enabled, ...(settings.bypass_enabled ? { reason: undefined } : {}) } : mode) }
    }
    if (args.operation === 'permission.settings_save') {
      if (refuse) throw refuse
      settings = { ...settings, ...args.body }
      return structuredClone(settings)
    }
    throw new Error('unknown')
  })
  return { tauri: { invoke }, requests, saves: () => requests.filter((request) => request.operation === 'permission.settings_save').map((request) => request.body) }
}

it('reads the runtime settings and saves each change on its own', async () => {
  const machine = runtime()
  const onsaved = vi.fn()
  render(PermissionSettings, { tauri: machine.tauri, runtime: 'box', machineName: 'Build box', onsaved })
  await screen.findByText('Default modes')
  expect(machine.requests[0]).toEqual({ runtime: 'box', operation: 'permission.settings', body: {} })
  expect(screen.getByText(/Meant for containers and disposable machines only/)).toBeInTheDocument()

  await fireEvent.click(screen.getByRole('button', { name: /^Default thread mode:/ }))
  await fireEvent.click(within(screen.getByRole('menu', { name: 'Default thread mode' })).getByRole('menuitemradio', { name: 'Plan first' }))
  await waitFor(() => expect(machine.saves()).toEqual([{ default_thread_mode: 'plan' }]))
  expect(onsaved).toHaveBeenCalled()

  await fireEvent.click(screen.getByRole('switch', { name: 'Allow bypass permissions' }))
  await waitFor(() => expect(machine.saves().at(-1)).toEqual({ bypass_enabled: true }))

  const wait = screen.getByRole('spinbutton', { name: 'Unattended wait (minutes)' })
  await fireEvent.input(wait, { target: { value: '30' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save wait' }))
  await waitFor(() => expect(machine.saves().at(-1)).toEqual({ unattended_timeout_seconds: 1800 }))

  await fireEvent.click(screen.getByRole('button', { name: 'Add rule' }))
  const form = screen.getByRole('form', { name: 'New permission rule' })
  await fireEvent.change(within(form).getByRole('combobox', { name: 'List' }), { target: { value: 'deny' } })
  await fireEvent.input(within(form).getByRole('textbox', { name: 'Command' }), { target: { value: 'rm -rf /' } })
  await fireEvent.input(within(form).getByRole('textbox', { name: 'Folder' }), { target: { value: '/work' } })
  await fireEvent.click(within(form).getByRole('button', { name: 'Add rule' }))
  await waitFor(() => expect(machine.saves().at(-1).deny.at(-1)).toEqual({ request_kind: 'command', resource: { type: 'command', arguments: ['bash', 'rm -rf /'], working_directory: '/work' } }))
})

it('names the refusal core gives', async () => {
  const machine = runtime('Choose an unattended wait between one minute and seven days.')
  render(PermissionSettings, { tauri: machine.tauri })
  const wait = await screen.findByRole('spinbutton', { name: 'Unattended wait (minutes)' })
  await fireEvent.input(wait, { target: { value: '45' } })
  await fireEvent.click(screen.getByRole('button', { name: 'Save wait' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('Choose an unattended wait between one minute and seven days.')
})
