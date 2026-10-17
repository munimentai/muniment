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
      const off = { available: false, reason: "Bypass is off. Turn it on in the runtime's permission settings." }
      return { ...modes, default_thread_mode: settings.default_thread_mode, default_agent_mode: settings.default_agent_mode, modes: modes.modes.map((mode) => mode.id === 'bypass' && !settings.bypass_enabled ? { ...mode, ...off } : mode) }
    }
    if (args.operation === 'permission.settings_save') {
      if (refuse) throw refuse
      settings = { ...settings, ...args.body }
      // Turning bypass off moves a default that names it to manual.
      if (args.body.bypass_enabled === false) {
        for (const key of ['default_thread_mode', 'default_agent_mode']) if (settings[key] === 'bypass' && !(key in args.body)) settings[key] = 'manual'
      }
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
  expect(screen.getByRole('button', { name: /^Default thread mode:/ })).toHaveTextContent('Bypass permissions')
  expect(screen.getByRole('button', { name: /^Default agent mode:/ })).toHaveTextContent('Bypass permissions')
  expect(screen.getByRole('switch', { name: 'Allow bypass permissions' })).toBeChecked()
  expect(screen.getByText(/Nothing else asks first/)).toBeInTheDocument()
  expect(screen.getByText(/Turn it off and new threads and agents start in Ask before changes/)).toBeInTheDocument()

  await fireEvent.click(screen.getByRole('button', { name: /^Default thread mode:/ }))
  await fireEvent.click(within(screen.getByRole('menu', { name: 'Default thread mode' })).getByRole('menuitemradio', { name: 'Plan first' }))
  await waitFor(() => expect(machine.saves()).toEqual([{ default_thread_mode: 'plan' }]))
  expect(onsaved).toHaveBeenCalled()

  // Turning bypass off moves the default agent mode, which names it, to manual.
  await fireEvent.click(screen.getByRole('switch', { name: 'Allow bypass permissions' }))
  await waitFor(() => expect(machine.saves().at(-1)).toEqual({ bypass_enabled: false }))
  await waitFor(() => expect(screen.getByRole('button', { name: /^Default agent mode:/ })).toHaveTextContent('Ask before changes'))
  expect(screen.getByRole('button', { name: /^Default thread mode:/ })).toHaveTextContent('Plan first')
  expect(screen.getByRole('switch', { name: 'Allow bypass permissions' })).not.toBeChecked()

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
