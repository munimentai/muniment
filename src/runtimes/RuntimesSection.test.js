import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import RuntimesSection from './RuntimesSection.svelte'
import RuntimeSwitcher from './RuntimeSwitcher.svelte'

afterEach(cleanup)

const LINK = `muniment://pair?v=1&id=018f0000-0000-7000-8000-000000000044&name=Lab&fp=${'00'.repeat(32)}&host=192.0.2.20%3A7443#${'ab'.repeat(16)}`
const local = { id: 'local', name: 'This computer', kind: 'local', version: '0.2.0', status: 'online', local: true, scopes: [], admin: true, updateOffered: false, sshHost: null }
const studio = { id: '018f0000-0000-7000-8000-000000000042', name: 'Studio', kind: 'desktop', version: '0.1.4', status: 'offline', local: false, scopes: ['run.write', 'thread.read'], admin: false, updateOffered: false, sshHost: null }
const buildBox = { id: '018f0000-0000-7000-8000-000000000043', name: 'Build box', kind: 'container', version: '0.1.0', status: 'online', local: false, scopes: ['run.write', 'runtime.admin', 'thread.read'], admin: true, updateOffered: true, sshHost: 'build-box' }

function runtimeList(runtimes = [local, studio, buildBox], selected = 'local') {
  return {
    appVersion: '0.2.0',
    deviceName: 'Laptop',
    selected,
    runtimes: runtimes.map((runtime) => ({ ...runtime, active: runtime.id === selected })),
  }
}

function tauri(handlers = {}) {
  let current = runtimeList()
  const invoke = vi.fn(async (command, payload) => {
    if (handlers[command]) return handlers[command](payload)
    if (command === 'runtime_list' || command === 'runtime_refresh') return current
    if (command === 'local_access_settings') return { settings: { enabled: false, listen: null, endpoints: [] }, overridden: false, listening: null, endpoints: [], error: null }
    if (command === 'local_devices') return { devices: [] }
    if (command === 'runtime_select') {
      current = runtimeList(undefined, payload.id)
      return current
    }
    throw new Error(`unexpected command: ${command}`)
  })
  return { invoke }
}

const listen = vi.fn(async () => () => {})

describe('Runtimes page', () => {
  it('lists this computer first, then each paired runtime with its kind, version and state', async () => {
    render(RuntimesSection, { tauri: tauri(), listen })
    const local = await screen.findByRole('group', { name: 'This computer' })
    expect(within(local).getByText('In use')).toBeInTheDocument()
    expect(within(local).queryByRole('button', { name: 'Rename This computer' })).toBeNull()
    expect(within(local).queryByRole('button', { name: 'Remove This computer' })).toBeNull()

    const studio = screen.getByRole('group', { name: 'Studio' })
    expect(within(studio).getByText('v0.1.4')).toBeInTheDocument()
    expect(within(studio).getByText('Desktop')).toBeInTheDocument()
    expect(within(studio).getByText('Offline')).toBeInTheDocument()
    expect(within(studio).getByText('Check that it is running and reachable.')).toBeInTheDocument()
    // An older runtime without the admin scope shows its version only.
    expect(within(studio).queryByRole('button', { name: 'Update Studio' })).toBeNull()

    const box = screen.getByRole('group', { name: 'Build box' })
    expect(within(box).getByText('ssh build-box')).toBeInTheDocument()
    expect(within(box).getByText('Container')).toBeInTheDocument()
    expect(within(box).getByRole('button', { name: 'Update Build box' })).toBeInTheDocument()
    expect(screen.getAllByRole('group').map((group) => group.getAttribute('aria-label'))).toEqual(['This computer', 'Studio', 'Build box'])
  })

  it('switches, renames and removes a runtime, asking once before removal', async () => {
    const bridge = tauri({
      runtime_rename: ({ name }) => runtimeList([local, studio, { ...buildBox, name }], buildBox.id),
      runtime_remove: () => ({ ...runtimeList([local, { ...buildBox, name: 'Lab' }], buildBox.id), notice: 'Studio still lists this device. Revoke it on Studio to end its access.' }),
    })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'Switch to Build box' }))
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_select', { id: buildBox.id })
    await waitFor(() => expect(within(screen.getByRole('group', { name: 'Build box' })).getByText('In use')).toBeInTheDocument())
    expect(screen.getByRole('button', { name: 'Switch to This computer' })).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'Rename Build box' }))
    const name = screen.getByRole('textbox', { name: 'Runtime name' })
    await fireEvent.input(name, { target: { value: 'Lab' } })
    await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_rename', { id: buildBox.id, name: 'Lab' })
    expect(await screen.findByRole('group', { name: 'Lab' })).toBeInTheDocument()

    await fireEvent.click(screen.getByRole('button', { name: 'Remove Studio' }))
    expect(bridge.invoke).not.toHaveBeenCalledWith('runtime_remove', expect.anything())
    expect(screen.getByText('Remove Studio?')).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }))
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_remove', { id: studio.id })
    await waitFor(() => expect(screen.queryByRole('group', { name: 'Studio' })).toBeNull())
    expect(screen.getByRole('status')).toHaveTextContent('Studio still lists this device. Revoke it on Studio to end its access.')
  })

  it('locks Switch while a reply runs', async () => {
    render(RuntimesSection, { tauri: tauri(), listen, switchDisabled: true })
    expect(await screen.findByRole('button', { name: 'Switch to Build box' })).toBeDisabled()
  })

  it('requests an update for one runtime and for all, naming each runtime that refused', async () => {
    const bridge = tauri({
      runtime_update: ({ id }) => ({ id, name: 'Build box', ok: true, version: '0.2.0', message: 'Update to 0.2.0 requested.' }),
      runtime_update_all: () => [
        { id: buildBox.id, name: 'Build box', ok: true, version: '0.2.0', message: 'Update to 0.2.0 requested.' },
        { id: studio.id, name: 'Studio', ok: false, version: null, message: 'The runtime is offline. Check that it is running and reachable.' },
      ],
    })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'Update Build box' }))
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_update', { id: buildBox.id })
    expect(await screen.findByRole('status')).toHaveTextContent('Build box: Update to 0.2.0 requested.')

    await fireEvent.click(screen.getByRole('button', { name: 'Update all' }))
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_update_all')
    expect(await screen.findByRole('alert')).toHaveTextContent('Studio: The runtime is offline. Check that it is running and reachable.')
  })

  it('shows no Update all when no runtime is older', async () => {
    const bridge = tauri({ runtime_list: () => runtimeList([local, studio]), runtime_refresh: () => runtimeList([local, studio]) })
    render(RuntimesSection, { tauri: bridge, listen })
    await screen.findByRole('group', { name: 'Studio' })
    expect(screen.queryByRole('button', { name: 'Update all' })).toBeNull()
  })

  it('pairs from a pasted link with this computer name and clears the link', async () => {
    let finish
    const bridge = tauri({
      runtime_pair_link: () => new Promise((resolve) => { finish = resolve }),
    })
    render(RuntimesSection, { tauri: bridge, listen })
    const pair = await screen.findByRole('button', { name: 'Pair' })
    expect(pair).toBeDisabled()
    const field = screen.getByRole('textbox', { name: 'Pairing link' })
    await fireEvent.input(field, { target: { value: `  ${LINK}  ` } })
    expect(screen.getByRole('textbox', { name: 'Name for this computer' })).toHaveAttribute('placeholder', 'Laptop')
    await fireEvent.click(pair)
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_pair_link', { link: LINK, deviceName: null })
    expect(await screen.findByText('Pairing with the runtime…')).toBeInTheDocument()
    expect(field).toBeDisabled()
    finish(runtimeList([local, studio, buildBox, { ...buildBox, id: '018f0000-0000-7000-8000-000000000044', name: 'Lab' }]))
    expect(await screen.findByRole('group', { name: 'Lab' })).toBeInTheDocument()
    expect(field).toHaveValue('')
  })

  it('shows why pairing failed and keeps the link for another try', async () => {
    const bridge = tauri({
      runtime_pair_link: () => { throw 'The runtime refused the link. It may be used or expired. Create a new link.' },
    })
    render(RuntimesSection, { tauri: bridge, listen })
    const field = await screen.findByRole('textbox', { name: 'Pairing link' })
    await fireEvent.input(field, { target: { value: LINK } })
    await fireEvent.click(screen.getByRole('button', { name: 'Pair' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('The runtime refused the link. It may be used or expired. Create a new link.')
    expect(field).toHaveValue(LINK)
  })

  it('pairs over SSH with the host, the remote command and the runtime port', async () => {
    const bridge = tauri({ runtime_pair_ssh: () => runtimeList() })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('radio', { name: 'Over SSH' }))
    const pair = screen.getByRole('button', { name: 'Pair over SSH' })
    expect(pair).toBeDisabled()
    expect(screen.getByRole('textbox', { name: 'Remote command' })).toHaveValue('muniment-server')
    expect(screen.getByRole('textbox', { name: 'Runtime port' })).toHaveValue('7443')
    await fireEvent.input(screen.getByRole('textbox', { name: 'SSH host' }), { target: { value: 'me@build-box' } })
    await fireEvent.input(screen.getByRole('textbox', { name: 'Remote command' }), { target: { value: 'docker exec muniment-runtime muniment-server' } })
    await fireEvent.input(screen.getByRole('textbox', { name: 'Runtime port' }), { target: { value: '70000' } })
    expect(pair).toBeDisabled()
    await fireEvent.input(screen.getByRole('textbox', { name: 'Runtime port' }), { target: { value: '7444' } })
    await fireEvent.input(screen.getByRole('textbox', { name: 'Name for this computer' }), { target: { value: 'Work laptop' } })
    await fireEvent.click(pair)
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_pair_ssh', { host: 'me@build-box', command: 'docker exec muniment-runtime muniment-server', port: 7444, deviceName: 'Work laptop' })
  })

  it('follows the runtime list the app announces', async () => {
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

describe('runtime switcher', () => {
  it('stays hidden while this computer is the only runtime', () => {
    render(RuntimeSwitcher, { list: runtimeList([local]) })
    expect(screen.queryByRole('button', { name: /Runtime:/ })).toBeNull()
  })

  it('names the runtime in use and switches from its menu', async () => {
    const onselect = vi.fn()
    const onmanage = vi.fn()
    render(RuntimeSwitcher, { list: runtimeList(undefined, buildBox.id), onselect, onmanage })
    const toggle = screen.getByRole('button', { name: 'Runtime: Build box. Switch runtime' })
    expect(toggle).toHaveTextContent('Build box')
    await fireEvent.click(toggle)
    const menu = screen.getByRole('menu', { name: 'Runtimes' })
    const items = within(menu).getAllByRole('menuitemradio')
    expect(items.map((item) => item.textContent.trim().replace(/\s+/g, ' '))).toEqual(['This computer Online', 'Studio Offline', 'Build box In use'])
    expect(items[2]).toHaveAttribute('aria-checked', 'true')
    await fireEvent.click(items[0])
    expect(onselect).toHaveBeenCalledWith(expect.objectContaining({ id: 'local' }))
    expect(screen.queryByRole('menu')).toBeNull()

    await fireEvent.click(toggle)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Manage runtimes' }))
    expect(onmanage).toHaveBeenCalledOnce()
  })

  it('shows a paired runtime that is not online and locks other runtimes during a reply', async () => {
    render(RuntimeSwitcher, { list: runtimeList([local, { ...studio, status: 'certificate_changed' }], studio.id), disabled: true })
    const toggle = screen.getByRole('button', { name: 'Runtime: Studio. Switch runtime' })
    expect(toggle).toHaveTextContent('Certificate changed')
    await fireEvent.click(toggle)
    const [computer, current] = screen.getAllByRole('menuitemradio')
    expect(computer).toBeDisabled()
    expect(current).not.toBeDisabled()
    await fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    expect(screen.queryByRole('menu')).toBeNull()
    expect(toggle).toHaveFocus()
  })
})
