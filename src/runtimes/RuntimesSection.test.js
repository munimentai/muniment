import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import RuntimesSection from './RuntimesSection.svelte'
import MachinePicker from './MachinePicker.svelte'

vi.mock('../feature-flags.js', () => ({ featureFlags: { cloud: true } }))

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

    // An older machine claims no update until it reports one.
    const box = screen.getByRole('group', { name: 'Build box' })
    expect(within(box).getByText('ssh build-box')).toBeInTheDocument()
    expect(within(box).getByText('Online')).toBeInTheDocument()
    expect(within(box).queryByText(/Updating/)).toBeNull()
    expect(screen.queryByRole('button', { name: /^Switch/ })).toBeNull()
    expect(screen.getByRole('button', { name: 'Update all' })).toBeEnabled()
    expect(machineRows().map((group) => group.getAttribute('aria-label'))).toEqual(['This computer', 'Studio', 'Build box'])
  })

  it('shows the update state each machine reports', async () => {
    const draining = { ...buildBox, updateStatus: { state: 'draining', targetVersion: '0.2.0', blockers: ['active_run', 'pending_permission_gate'] } }
    const failed = { ...studio, id: '018f0000-0000-7000-8000-000000000045', name: 'Lab', status: 'online', updateStatus: { state: 'failed', targetVersion: '0.2.0', blockers: [], error: 'Version 0.2.0 did not report healthy.' } }
    const manual = { ...buildBox, id: '018f0000-0000-7000-8000-000000000046', name: 'Rack', behind: true, updateOffered: false, updateCapability: 'manual', updateTarget: '0.2.0' }
    const asked = { ...buildBox, id: '018f0000-0000-7000-8000-000000000047', name: 'Mini box', updateRequested: true }
    const bridge = tauri({ runtime_list: () => runtimeList([local, draining, failed, manual, asked]), runtime_refresh: () => runtimeList([local, draining, failed, manual, asked]) })
    render(RuntimesSection, { tauri: bridge, listen })
    const finishing = await within(await screen.findByRole('group', { name: 'Build box' })).findByText('Finishing current work')
    expect(finishing).toHaveAttribute('data-tooltip', 'Waiting for a reply in progress and a permission request.')
    expect(within(screen.getByRole('group', { name: 'Lab' })).getByText('Update failed')).toHaveAttribute('data-tooltip', 'Version 0.2.0 did not report healthy.')
    expect(within(screen.getByRole('group', { name: 'Rack' })).getByText('v0.2.0 available, update manually')).toBeInTheDocument()
    expect(within(screen.getByRole('group', { name: 'Mini box' })).getByText('Update requested')).toBeInTheDocument()
  })

  it('updates one machine from its menu', async () => {
    const bridge = tauri({ runtime_update: () => ({ id: buildBox.id, name: 'Build box', ok: true, version: '0.2.0', message: 'Update to 0.2.0 requested.' }) })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'More for Build box' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Update' }))
    expect(bridge.invoke).toHaveBeenCalledWith('runtime_update', { id: buildBox.id })
    expect(await screen.findByText('Update to 0.2.0 requested.')).toBeInTheDocument()
    // A machine this device may not update offers no Update.
    await fireEvent.click(screen.getByRole('button', { name: 'More for Studio' }))
    expect(screen.queryByRole('menuitem', { name: 'Update' })).toBeNull()
  })

  it('updates every machine that is behind, names each skip and updates this computer last', async () => {
    const order = []
    const bridge = tauri({
      runtime_update_all: () => {
        order.push('machines')
        return {
          results: [{ id: buildBox.id, name: 'Build box', ok: true, version: '0.2.0', message: 'Update to 0.2.0 requested.' }],
          skipped: [
            { id: studio.id, name: 'Studio', reason: 'offline' },
            { id: 'reader', name: 'Reader', reason: 'no_permission' },
            { id: 'rack', name: 'Rack', reason: 'manual' },
            { id: 'lab', name: 'Lab', reason: 'current' },
          ],
        }
      },
      app_update_prepare: () => { order.push('prepare'); return '0.3.0' },
      app_update_interruptions: () => { order.push('count'); return 1 },
      app_update_install: () => { order.push('install'); return null },
    })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'Update all' }))
    const dialog = await screen.findByRole('dialog', { name: 'Install update 0.3.0?' })
    expect(dialog).toHaveTextContent('Installing now stops 1 running reply. It resumes after the update.')
    expect(screen.getByText('Update requested on Build box.')).toBeInTheDocument()
    expect(screen.getByText('Skipped: Studio (offline), Reader (no permission), Rack (manual install), Lab (already current).')).toBeInTheDocument()
    expect(order).toEqual(['machines', 'prepare', 'count'])
    await fireEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    await waitFor(() => expect(order).toEqual(['machines', 'prepare', 'count', 'install']))
  })

  it('names this computer as current when no app update waits', async () => {
    const bridge = tauri({
      runtime_update_all: () => ({ results: [], skipped: [] }),
      app_update_prepare: () => null,
    })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'Update all' }))
    expect(await screen.findByText('Skipped: This computer (already current).')).toBeInTheDocument()
    expect(bridge.invoke).not.toHaveBeenCalledWith('app_update_install')
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
    expect(toggle).toHaveAttribute('aria-expanded', 'true')
    expect(toggle.querySelector('[data-icon=chevrons-down-up]')).not.toBeNull()
    // The menu is a composer panel, so it opens above the composer like the others.
    const menu = screen.getByRole('menu', { name: 'Machines' })
    expect(menu).toHaveAttribute('data-composer-panel')
    const items = within(menu).getAllByRole('menuitemradio')
    // A globe marks each machine's state in place of a word: green while it answers, red with a slash when it does not.
    expect(items.map((item) => item.textContent.trim())).toEqual(['This computer', 'Studio', 'Build box'])
    expect(within(items[0]).getByRole('img', { name: 'Online' }).querySelector('[data-icon=globe]')).not.toBeNull()
    expect(within(items[1]).getByRole('img', { name: 'Offline' }).querySelector('[data-icon=globe-off]')).not.toBeNull()
    expect(within(items[2]).getByRole('img', { name: 'Online' })).toHaveClass('online')
    expect(items[0]).toHaveAttribute('aria-checked', 'true')
    await fireEvent.click(items[2])
    expect(onselect).toHaveBeenCalledWith(expect.objectContaining({ id: buildBox.id }))
    expect(screen.queryByRole('menu')).toBeNull()
    expect(toggle.querySelector('[data-icon=chevrons-up-down]')).not.toBeNull()

    // An offline machine takes no new thread, so it opens Settings → Machines.
    await fireEvent.click(toggle)
    await fireEvent.click(screen.getByRole('menuitemradio', { name: 'Studio Offline' }))
    expect(onselect).toHaveBeenCalledOnce()
    expect(onmanage).toHaveBeenCalledOnce()
    expect(screen.queryByRole('menu')).toBeNull()

    await fireEvent.click(toggle)
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Add a machine' }))
    expect(onmanage).toHaveBeenCalledTimes(2)
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

  it('keeps shared connections in MuniCloud for a signed-in account', async () => {
    let state = { shareModels: true, shareViaCloud: false }
    const limited = { ...studio, status: 'online', sharing: 'limit' }
    const current = () => ({ ...runtimeList([local, limited]), ...state })
    const bridge = tauri({
      auth_status: () => ({ signed_in: true, subject: 'owner' }),
      runtime_list: current,
      runtime_refresh: current,
      runtime_share_models: ({ share, cloud }) => { state = { shareModels: share, shareViaCloud: cloud ?? state.shareViaCloud }; return current() },
    })
    render(RuntimesSection, { tauri: bridge, listen })
    const cloud = await screen.findByRole('switch', { name: 'Keep them in MuniCloud' })
    expect(screen.getByRole('note')).toHaveTextContent('Shared subscriptions depend on this computer.')
    await fireEvent.click(cloud)
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith('runtime_share_models', { share: true, cloud: true }))
    await waitFor(() => expect(screen.getByRole('note')).toHaveTextContent('MuniCloud renews shared sign-ins'))
    const desk = screen.getByRole('group', { name: 'Studio' })
    expect(within(desk).getByText('Your account shares with as many machines as it may. Remove one to add this machine.')).toBeInTheDocument()
  })

  it('hides MuniCloud from a computer that is not signed in', async () => {
    render(RuntimesSection, { tauri: tauri({ auth_status: () => ({ signed_in: false }) }), listen })
    await screen.findByRole('switch', { name: 'Share model connections' })
    expect(screen.queryByRole('switch', { name: 'Keep them in MuniCloud' })).toBeNull()
  })
})
