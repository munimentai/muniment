import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import RuntimesSection from './RuntimesSection.svelte'
import MachinePicker from './MachinePicker.svelte'

afterEach(cleanup)

const LINK = `muniment://pair?v=1&id=018f0000-0000-7000-8000-000000000044&name=Lab&fp=${'00'.repeat(32)}&host=192.0.2.20%3A7443#${'ab'.repeat(16)}`
const local = { id: 'local', name: 'This computer', kind: 'local', version: '0.2.0', status: 'online', local: true, scopes: [], admin: true, updateOffered: false, sshHost: null }
const studio = { id: '018f0000-0000-7000-8000-000000000042', name: 'Studio', kind: 'desktop', version: '0.1.4', status: 'offline', local: false, scopes: ['run.write', 'thread.read'], admin: false, updateOffered: false, sshHost: null }
const buildBox = { id: '018f0000-0000-7000-8000-000000000043', name: 'Build box', kind: 'container', version: '0.1.0', status: 'online', local: false, scopes: ['run.write', 'runtime.update', 'thread.read'], admin: false, updateOffered: true, sshHost: 'build-box' }

function runtimeList(runtimes = [local, studio, buildBox], selected = 'local') {
  return {
    appVersion: '0.2.0',
    deviceName: 'Laptop',
    selected,
    runtimes: runtimes.map((runtime) => ({ ...runtime, active: runtime.id === selected })),
  }
}

function tauri(handlers = {}) {
  const invoke = vi.fn(async (command, payload) => {
    if (handlers[command]) return handlers[command](payload)
    if (command === 'runtime_list' || command === 'runtime_refresh') return runtimeList()
    if (command === 'local_access_settings') return { settings: { enabled: false, listen: null, endpoints: [] }, overridden: false, listening: null, endpoints: [], error: null }
    if (command === 'local_devices') return { devices: [] }
    throw new Error(`unexpected command: ${command}`)
  })
  return { invoke }
}

const listen = vi.fn(async () => () => {})
const machineRows = () => within(screen.getByRole('heading', { name: /^Machines/ }).closest('section')).getAllByRole('group')

describe('Machines page', () => {
  it('lists this computer first, then each paired machine with its kind, version and state', async () => {
    render(RuntimesSection, { tauri: tauri(), listen })
    const computer = await screen.findByRole('group', { name: 'This computer' })
    expect(within(computer).getByText('Online')).toBeInTheDocument()
    expect(within(computer).queryByRole('button', { name: 'More for This computer' })).toBeNull()

    const desk = screen.getByRole('group', { name: 'Studio' })
    expect(within(desk).getByText('v0.1.4')).toBeInTheDocument()
    expect(within(desk).getByText('Desktop')).toBeInTheDocument()
    expect(within(desk).getByText('Offline')).toBeInTheDocument()
    expect(within(desk).getByText('Check that it is running and reachable.')).toBeInTheDocument()

    // An older machine this device may update shows that it updates on its own.
    const box = screen.getByRole('group', { name: 'Build box' })
    expect(within(box).getByText('ssh build-box')).toBeInTheDocument()
    expect(within(box).getByText('Updating to v0.2.0')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /^Switch/ })).toBeNull()
    expect(screen.queryByRole('button', { name: /^Update/ })).toBeNull()
    expect(machineRows().map((group) => group.getAttribute('aria-label'))).toEqual(['This computer', 'Studio', 'Build box'])
  })

  it('renames and removes a machine from its menu, asking once before removal', async () => {
    const bridge = tauri({
      runtime_rename: ({ name }) => runtimeList([local, studio, { ...buildBox, name }]),
      runtime_remove: () => ({ ...runtimeList([local, { ...buildBox, name: 'Lab' }]), notice: 'Studio still lists this device. Revoke it on Studio to end its access.' }),
    })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'More for Build box' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }))
    const name = screen.getByRole('textbox', { name: 'Machine name' })
    await fireEvent.input(name, { target: { value: 'Lab' } })
    await fireEvent.click(within(screen.getByRole('form', { name: 'Rename Build box' })).getByRole('button', { name: 'Save' }))
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_rename', { id: buildBox.id, name: 'Lab' })
    expect(await screen.findByRole('group', { name: 'Lab' })).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'More for Studio' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Remove' }))
    expect(bridge.invoke).not.toHaveBeenCalledWith('runtime_remove', expect.anything())
    expect(screen.getByText('Remove Studio?')).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }))
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_remove', { id: studio.id })
    await waitFor(() => expect(screen.queryByRole('group', { name: 'Studio' })).toBeNull())
    expect(screen.getByRole('status')).toHaveTextContent('Studio still lists this device. Revoke it on Studio to end its access.')
  })

  it('adds a machine from a pasted link with this computer name and clears the link', async () => {
    let finish
    const bridge = tauri({
      runtime_pair_link: () => new Promise((resolve) => { finish = resolve }),
    })
    render(RuntimesSection, { tauri: bridge, listen })
    const add = await screen.findByRole('button', { name: 'Add' })
    expect(add).toBeDisabled()
    const field = screen.getByRole('textbox', { name: 'Pairing link' })
    await fireEvent.input(field, { target: { value: `  ${LINK}  ` } })
    await waitFor(() => expect(add).not.toBeDisabled())
    await fireEvent.click(add)
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith('runtime_pair_link', { link: LINK, deviceName: 'Laptop' }))
    expect(await screen.findByText('Connecting…')).toBeInTheDocument()
    expect(field).toBeDisabled()
    finish(runtimeList([local, studio, buildBox, { ...buildBox, id: '018f0000-0000-7000-8000-000000000044', name: 'Lab' }]))
    expect(await screen.findByRole('group', { name: 'Lab' })).toBeInTheDocument()
    expect(field).toHaveValue('')
    expect(screen.getByText('Machine added.')).toBeInTheDocument()
  })

  it('shows why pairing failed and keeps the link for another try', async () => {
    const bridge = tauri({
      runtime_pair_link: () => { throw 'The runtime refused the link. It may be used or expired. Create a new link.' },
    })
    render(RuntimesSection, { tauri: bridge, listen })
    const field = await screen.findByRole('textbox', { name: 'Pairing link' })
    await fireEvent.input(field, { target: { value: LINK } })
    await fireEvent.click(screen.getByRole('button', { name: 'Add' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('The runtime refused the link. It may be used or expired. Create a new link.')
    expect(field).toHaveValue(LINK)
  })

  it('adds a machine over SSH with the host, the remote command and the runtime port', async () => {
    const bridge = tauri({ runtime_pair_ssh: () => runtimeList() })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'Use SSH instead' }))
    const add = screen.getByRole('button', { name: 'Add' })
    expect(add).toBeDisabled()
    expect(screen.getByRole('textbox', { name: 'Remote command' })).toHaveValue('muniment-server')
    expect(screen.getByRole('textbox', { name: 'Runtime port' })).toHaveValue('7443')
    await fireEvent.input(screen.getByRole('textbox', { name: 'SSH host' }), { target: { value: 'me@build-box' } })
    await fireEvent.input(screen.getByRole('textbox', { name: 'Remote command' }), { target: { value: 'docker exec muniment-runtime muniment-server' } })
    await fireEvent.input(screen.getByRole('textbox', { name: 'Runtime port' }), { target: { value: '70000' } })
    expect(add).toBeDisabled()
    await fireEvent.input(screen.getByRole('textbox', { name: 'Runtime port' }), { target: { value: '7444' } })
    await fireEvent.click(add)
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_pair_ssh', { host: 'me@build-box', command: 'docker exec muniment-runtime muniment-server', port: 7444, deviceName: 'Laptop' })
    await fireEvent.click(screen.getByRole('button', { name: 'Use a pairing link instead' }))
    expect(screen.getByRole('textbox', { name: 'Pairing link' })).toBeInTheDocument()
  })

  it('follows the machine list the app announces', async () => {
    let announce
    const listening = vi.fn(async (event, listener) => { if (event === 'runtimes-changed') announce = listener; return () => {} })
    render(RuntimesSection, { tauri: tauri(), listen: listening })
    await screen.findByRole('group', { name: 'Studio' })
    await waitFor(() => expect(announce).toBeTypeOf('function'))
    announce({ payload: runtimeList([local, { ...studio, status: 'revoked' }]) })
    expect(await within(screen.getByRole('group', { name: 'Studio' })).findByText('Access revoked')).toBeInTheDocument()
    expect(screen.getByText('Pair it again with a new link.')).toBeInTheDocument()
  })
})

describe('machine picker', () => {
  it('stays hidden while this computer is the only machine', () => {
    render(MachinePicker, { list: runtimeList([local]) })
    expect(screen.queryByRole('button', { name: /^Machine:/ })).toBeNull()
  })

  it('names the machine a new thread starts on and picks another from its menu', async () => {
    const onselect = vi.fn()
    const onmanage = vi.fn()
    render(MachinePicker, { list: runtimeList(undefined, 'local'), onselect, onmanage })
    const toggle = screen.getByRole('button', { name: 'Machine: This computer. Choose a machine' })
    await fireEvent.click(toggle)
    const menu = screen.getByRole('menu', { name: 'Machines' })
    const items = within(menu).getAllByRole('menuitemradio')
    expect(items.map((item) => item.textContent.trim().replace(/\s+/g, ' '))).toEqual(['This computer Online', 'Studio Offline', 'Build box Online'])
    expect(items[0]).toHaveAttribute('aria-checked', 'true')
    // An offline machine takes no new thread.
    expect(items[1]).toBeDisabled()
    await fireEvent.click(items[2])
    expect(onselect).toHaveBeenCalledWith(expect.objectContaining({ id: buildBox.id }))
    expect(screen.queryByRole('menu')).toBeNull()

    await fireEvent.click(toggle)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add a machine' }))
    expect(onmanage).toHaveBeenCalledOnce()
  })

  it('shows the machine an open thread lives on without a menu', () => {
    render(MachinePicker, { list: runtimeList(undefined, 'local'), locked: buildBox.id })
    expect(screen.queryByRole('button', { name: /^Machine:/ })).toBeNull()
    expect(screen.getByRole('note', { name: 'This thread stays on Build box' })).toHaveTextContent('Build box')
  })

  it('closes its menu with Escape and returns focus to the chip', async () => {
    render(MachinePicker, { list: runtimeList(undefined, 'local') })
    const toggle = screen.getByRole('button', { name: 'Machine: This computer. Choose a machine' })
    await fireEvent.click(toggle)
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    expect(screen.queryByRole('menu')).toBeNull()
    expect(toggle).toHaveFocus()
  })
})

describe('Model sharing', () => {
  it('turns sharing on under the machine list and shows which machines use it', async () => {
    const shared = { ...buildBox, sharing: 'shared' }
    const reader = { ...studio, sharing: 'no_access' }
    let on = false
    const current = () => ({ ...runtimeList(on ? [local, reader, shared] : [local, studio, buildBox]), shareModels: on })
    const bridge = tauri({
      runtime_list: current,
      runtime_refresh: current,
      runtime_share_models: ({ share }) => { on = share; return current() },
    })
    render(RuntimesSection, { tauri: bridge, listen })
    const toggle = await screen.findByRole('switch', { name: 'Share model connections' })
    expect(toggle).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('note')).toHaveTextContent('Shared subscriptions depend on this computer.')

    await fireEvent.click(toggle)
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith('runtime_share_models', { share: true }))
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'true'))
    const box = screen.getByRole('group', { name: 'Build box' })
    expect(within(box).getByText("Uses this computer's models")).toBeInTheDocument()
    const desk = screen.getByRole('group', { name: 'Studio' })
    expect(within(desk).getByText('Check that it is running and reachable.')).toBeInTheDocument()
  })

  it('asks a machine this device may not run on to pair again', async () => {
    const reader = { ...studio, status: 'online', scopes: ['thread.read'], sharing: 'no_access' }
    render(RuntimesSection, { tauri: tauri({ runtime_list: () => ({ ...runtimeList([local, reader]), shareModels: true }), runtime_refresh: () => ({ ...runtimeList([local, reader]), shareModels: true }) }), listen })
    const desk = await screen.findByRole('group', { name: 'Studio' })
    expect(within(desk).getByText("Pair it again with run access to share this computer's models.")).toBeInTheDocument()
  })
})
