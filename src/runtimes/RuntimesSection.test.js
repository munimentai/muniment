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
const machineRows = () => within(screen.getByRole('region', { name: 'Machine list' })).getAllByRole('group')

describe('Machines page', () => {
  it('lists the account\'s devices and the paired phone in the one list, and pairs or removes a phone from it', async () => {
    let pair = null
    const handlers = {
      auth_devices: () => [
        { device_id: 'd0', platform: 'macos', current: true, revoked_at: null, last_active_at: '2026-10-17T10:00:00Z' },
        { device_id: 'd1', platform: 'windows', current: false, revoked_at: null, last_active_at: '2026-10-17T09:00:00Z' },
        { device_id: 'd2', platform: 'ios', current: false, revoked_at: null, last_active_at: '2026-10-17T08:00:00Z' },
        { device_id: 'd3', platform: 'linux', current: false, revoked_at: '2026-10-01T00:00:00Z', last_active_at: '2026-09-01T00:00:00Z' },
      ],
      auth_pairing_status: () => ({ pair }),
      auth_pairing_challenge: () => ({ qr_svg: '<svg></svg>', expires_at: new Date(Date.now() + 60_000).toISOString() }),
      auth_pairing_revoke: () => { pair = null },
      app_update_prepare: () => null,
      agent_request: () => { throw new Error('no fleet') },
    }
    const { unmount } = render(RuntimesSection, { tauri: tauri(handlers), listen })
    await waitFor(() => expect(machineRows().map((group) => group.getAttribute('aria-label'))).toEqual(['This computer', 'Studio', 'Build box', 'Windows', 'iOS']))
    // Add machine offers a phone while none is paired.
    await fireEvent.click(screen.getByRole('button', { name: 'Add machine' }))
    expect(await screen.findByRole('button', { name: 'Pair a phone' })).toBeInTheDocument()
    unmount()

    pair = { pair_id: 'p1', mobile_device_id: 'm1', created_at: '2026-10-16T08:00:00Z', remote_control: 'ok' }
    render(RuntimesSection, { tauri: tauri(handlers), listen })
    // Status reads keep their two-second spacing across mounts.
    await waitFor(() => expect(machineRows().map((group) => group.getAttribute('aria-label'))).toEqual(['This computer', 'Studio', 'Build box', 'Phone', 'Windows']), { timeout: 4000 })
    await fireEvent.click(screen.getByRole('button', { name: 'Add machine' }))
    expect(screen.queryByRole('button', { name: 'Pair a phone' })).toBeNull()
    // Remove on the phone's row revokes the pairing.
    await fireEvent.click(within(screen.getByRole('group', { name: 'Phone' })).getByRole('button', { name: 'More for Phone' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Remove' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }))
    await waitFor(() => expect(machineRows().map((group) => group.getAttribute('aria-label'))).toEqual(['This computer', 'Studio', 'Build box', 'Windows', 'iOS']))
    expect(await screen.findByText('Removed Phone.')).toBeInTheDocument()
  })


  it('shows a phone that must pair again on its row and pairs it again from the row menu', async () => {
    let pair = { pair_id: 'p1', mobile_device_id: 'm1', created_at: '2026-10-16T08:00:00Z', remote_control: 'pair_again' }
    const challenge = vi.fn(() => ({ qr_svg: '<svg></svg>', expires_at: new Date(Date.now() + 60_000).toISOString() }))
    const handlers = {
      auth_devices: () => [],
      auth_pairing_status: () => ({ pair }),
      auth_pairing_challenge: challenge,
      auth_pairing_revoke: () => { pair = null },
      app_update_prepare: () => null,
      agent_request: () => { throw new Error('no fleet') },
    }
    render(RuntimesSection, { tauri: tauri(handlers), listen })
    const row = await screen.findByRole('group', { name: 'Phone' }, { timeout: 4000 })
    expect(within(row).getByText('Pair again')).toBeInTheDocument()
    expect(within(row).getByText('This phone cannot open sessions from this computer until it pairs again.')).toBeInTheDocument()
    await fireEvent.click(within(row).getByRole('button', { name: 'More for Phone' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Pair again' }))
    await waitFor(() => expect(challenge).toHaveBeenCalled())
    await waitFor(() => expect(screen.queryByRole('group', { name: 'Phone' })).toBeNull())
  })

  it('keeps a machine that was added but not linked in view while sharing changes', async () => {
    const bridge = tauri({
      runtime_pair_link: () => ({ ...runtimeList(), notice: 'The machine is paired but not linked: The exchange failed.\nMalformedFrame' }),
      local_device_revoke: () => null,
      app_update_prepare: () => null,
      auth_devices: () => [],
      auth_pairing_status: () => ({ pair: null }),
      agent_request: ({ operation, body }) => operation === 'fleet.credentials' ? { share_models: body?.share_models ?? false } : { members: [] },
    })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'Add machine' }))
    await fireEvent.input(screen.getByRole('textbox', { name: 'Link from the other machine' }), { target: { value: LINK } })
    await fireEvent.click(screen.getByRole('button', { name: 'Add' }))
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('The machine is paired but not linked: The exchange failed.')
    expect(within(alert).getByText('Show log')).toBeInTheDocument()
    const share = screen.getByRole('switch', { name: 'Share model connections' })
    await waitFor(() => expect(share).toBeEnabled())
    await fireEvent.click(share)
    await waitFor(() => expect(share).toHaveAttribute('aria-checked', 'true'))
    expect(screen.getByRole('alert')).toHaveTextContent('paired but not linked')
  })

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
    expect(finishing.closest('[data-tooltip]')).toHaveAttribute('data-tooltip', 'Waiting for a reply in progress and a permission request.')
    expect(within(screen.getByRole('group', { name: 'Lab' })).getByText('Update failed').closest('[data-tooltip]')).toHaveAttribute('data-tooltip', 'Version 0.2.0 did not report healthy.')
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

  it('shows a failed update\'s first line inline with its log behind Show log', async () => {
    const bridge = tauri({ runtime_update: () => ({ id: buildBox.id, name: 'Build box', ok: false, message: 'The update did not install.\ninstaller exited with status 3' }) })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'More for Build box' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Update' }))
    const alert = await screen.findByRole('alert')
    expect(alert).toHaveTextContent('The update did not install.')
    expect(within(alert).getByText('Show log')).toBeInTheDocument()
    expect(within(alert).getByText('installer exited with status 3')).toBeInTheDocument()
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
    // The page asks once on open whether an update waits, then again for Update all.
    expect(order).toEqual(['prepare', 'machines', 'prepare', 'count'])
    await fireEvent.click(screen.getByRole('button', { name: 'Install and restart' }))
    await waitFor(() => expect(order).toEqual(['prepare', 'machines', 'prepare', 'count', 'install']))
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
    expect(screen.getByText('It leaves your machines and loses access to this computer. Its threads stay on it.')).toBeInTheDocument()
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
    await fireEvent.click(await screen.findByRole('button', { name: 'Add machine' }))
    const add = await screen.findByRole('button', { name: 'Add' })
    expect(add).toBeDisabled()
    const field = screen.getByRole('textbox', { name: 'Link from the other machine' })
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

  it('shows why adding failed and keeps the link for another try', async () => {
    const bridge = tauri({
      runtime_pair_link: () => { throw 'The runtime refused the link. It may be used or expired. Create a new link.' },
    })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'Add machine' }))
    const field = await screen.findByRole('textbox', { name: 'Link from the other machine' })
    await fireEvent.input(field, { target: { value: LINK } })
    await fireEvent.click(screen.getByRole('button', { name: 'Add' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('The runtime refused the link. It may be used or expired. Create a new link.')
    expect(field).toHaveValue(LINK)
  })

  it('adds a machine over SSH with the host, the remote command and the runtime port', async () => {
    const bridge = tauri({ runtime_pair_ssh: () => runtimeList() })
    render(RuntimesSection, { tauri: bridge, listen })
    await fireEvent.click(await screen.findByRole('button', { name: 'Add machine' }))
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
    await fireEvent.click(screen.getByRole('button', { name: 'Paste a link instead' }))
    expect(screen.getByRole('textbox', { name: 'Link from the other machine' })).toBeInTheDocument()
  })

  it('follows the machine list the app announces', async () => {
    let announce
    const listening = vi.fn(async (event, listener) => { if (event === 'runtimes-changed') announce = listener; return () => {} })
    render(RuntimesSection, { tauri: tauri(), listen: listening })
    await screen.findByRole('group', { name: 'Studio' })
    await waitFor(() => expect(announce).toBeTypeOf('function'))
    announce({ payload: runtimeList([local, { ...studio, status: 'revoked' }]) })
    expect(await within(screen.getByRole('group', { name: 'Studio' })).findByText('Access revoked')).toBeInTheDocument()
    expect(screen.getByText('Add it again with a new code.')).toBeInTheDocument()
  })
})

describe('one list of machines', () => {
  const CORE_LOCAL = '018f0000-0000-7000-8000-0000000000c1'
  const CORE_BOX = '018f0000-0000-7000-8000-0000000000c3'
  const CORE_POD = '018f0000-0000-7000-8000-0000000000c6'
  const box = { ...buildBox, status: 'online', admin: true, updateOffered: false }
  const heartbeat = new Date(Date.now() - 3 * 3600_000).toISOString()
  const views = {
    local: { self: CORE_LOCAL, members: [
      { runtime_id: CORE_LOCAL, self: true, online: true },
      { runtime_id: CORE_BOX, runtime_name: 'Build box', form_factor: 'container', online: true, link: { state: 'connected' } },
      { runtime_id: CORE_POD, runtime_name: 'Pod', form_factor: 'container', online: false, heartbeat_at: heartbeat, link: { state: 'retrying' } },
    ] },
    [box.id]: { self: CORE_BOX, members: [
      { runtime_id: CORE_BOX, self: true, online: true },
      { runtime_id: CORE_LOCAL, runtime_name: 'Laptop', online: true, link: { state: 'connected' } },
    ] },
  }
  const devices = [
    { device_id: 'd-phone', name: 'Pocket', kind: 'mobile', scopes: ['run.write', 'thread.read'], created_at: '2030-01-01T00:00:00Z', last_seen_at: null },
    { device_id: 'd-box', name: 'Build box', kind: 'desktop-client', scopes: ['run.write', 'runtime.admin', 'thread.read'], created_at: '2030-01-01T00:00:00Z', last_seen_at: null },
    { device_id: 'd-peer', name: 'Pod', kind: 'swarm-peer', scopes: ['swarm.peer'], created_at: '2030-01-01T00:00:00Z', last_seen_at: null },
  ]
  function fleetBridge(calls) {
    let listed = devices
    return tauri({
      runtime_list: () => runtimeList([local, studio, box]),
      runtime_refresh: () => runtimeList([local, studio, box]),
      local_devices: () => ({ devices: listed }),
      local_device_revoke: ({ deviceId }) => { calls.push(['revoke', deviceId]); listed = listed.filter((device) => device.device_id !== deviceId); return true },
      runtime_remove: ({ id }) => { calls.push(['remove', id]); return runtimeList([local, studio]) },
      agent_request: ({ runtime, operation, body }) => {
        if (operation === 'swarm.members') return views[runtime]
        if (operation === 'swarm.leave') { calls.push(['leave', runtime, body.runtime_id]); return { removed: true } }
        if (operation === 'fleet.credentials') return { share_models: false }
        return {}
      },
    })
  }

  it('lists every machine once: reached, linked through the fleet, and using this computer', async () => {
    render(RuntimesSection, { tauri: fleetBridge([]), listen })
    await screen.findByRole('group', { name: 'Pod' })
    await waitFor(() => expect(machineRows().map((group) => group.getAttribute('aria-label'))).toEqual(['This computer', 'Studio', 'Build box', 'Pod', 'Pocket']))
    const pod = screen.getByRole('group', { name: 'Pod' })
    expect(within(pod).getByText('Container')).toBeInTheDocument()
    expect(within(pod).getByText('Offline')).toBeInTheDocument()
    expect(within(pod).getByText('Seen 3h ago')).toBeInTheDocument()
    const phone = screen.getByRole('group', { name: 'Pocket' })
    expect(within(phone).getByText('Phone')).toBeInTheDocument()
    expect(within(phone).getByText('Limited')).toBeInTheDocument()
    expect(within(phone).getByText('Never seen')).toBeInTheDocument()
    // A machine with admin access has no Limited chip, and no word names a fleet.
    expect(within(screen.getByRole('group', { name: 'Build box' })).queryByText('Limited')).toBeNull()
    expect(document.body.textContent).not.toMatch(/fleet|Fleet|Pair\b|Join/)
    // No update waits, so Update all stays off the page.
    expect(screen.queryByRole('button', { name: 'Update all' })).toBeNull()
  })

  it('removes a machine from every link and its access in one step', async () => {
    const calls = []
    render(RuntimesSection, { tauri: fleetBridge(calls), listen })
    await screen.findByRole('group', { name: 'Pod' })
    await waitFor(() => expect(screen.getByRole('button', { name: 'More for Build box' })).toBeEnabled())
    await fireEvent.click(screen.getByRole('button', { name: 'More for Build box' }))
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Remove' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }))
    await waitFor(() => expect(calls).toEqual([['leave', 'local', CORE_BOX], ['revoke', 'd-box'], ['remove', box.id]]))
    expect(await screen.findByText('Removed Build box.')).toBeInTheDocument()

    calls.length = 0
    await fireEvent.click(screen.getByRole('button', { name: 'More for Pocket' }))
    expect(screen.queryByRole('menuitem', { name: 'Rename' })).toBeNull()
    await fireEvent.click(screen.getByRole('menuitem', { name: 'Remove' }))
    await fireEvent.click(screen.getByRole('button', { name: 'Remove' }))
    await waitFor(() => expect(calls).toEqual([['revoke', 'd-phone']]))
    await waitFor(() => expect(screen.queryByRole('group', { name: 'Pocket' })).toBeNull())
  })

  it('opens this computer\'s network, offline and log settings from its row', async () => {
    render(RuntimesSection, { tauri: fleetBridge([]), listen })
    const open = await screen.findByRole('button', { name: 'Settings for This computer' })
    expect(screen.queryByRole('switch', { name: 'Accept connections' })).toBeNull()
    await fireEvent.click(open)
    expect(await screen.findByRole('switch', { name: 'Accept connections' })).toBeInTheDocument()
    expect(open).toHaveAttribute('aria-expanded', 'true')
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
  // Core keeps the sharing switch: `fleet.credentials` answers and sets it.
  function fleet(state) {
    return ({ runtime, operation, body }) => {
      if (operation !== 'fleet.credentials') throw new Error(`unexpected operation: ${operation}`)
      expect(runtime).toBe('local')
      if (body.share_models !== undefined) state.shareModels = body.share_models
      return { share_models: state.shareModels, role: 'full', removed: false, secrets: [], audit: [] }
    }
  }

  it('reads the switch from core and turns sharing on through fleet.credentials', async () => {
    const state = { shareModels: false }
    const bridge = tauri({ agent_request: fleet(state) })
    render(RuntimesSection, { tauri: bridge, listen })
    const toggle = await screen.findByRole('switch', { name: 'Share model connections' })
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'fleet.credentials', body: {} }))
    await waitFor(() => expect(toggle).toBeEnabled())
    expect(toggle).toHaveAttribute('aria-checked', 'false')
    // The page holds no banner: the explanation sits behind the row's info control.
    expect(screen.queryByRole('note')).toBeNull()
    expect(screen.getByRole('tooltip', { hidden: true })).toHaveTextContent('Shared subscriptions depend on this computer')

    await fireEvent.click(toggle)
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'fleet.credentials', body: { share_models: true } }))
    expect(state.shareModels).toBe(true)
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'true'))
    expect(bridge.invoke).not.toHaveBeenCalledWith('runtime_share_models', expect.anything())
    // Machine rows carry no share state of their own.
    expect(screen.queryByText("Uses this computer's models")).toBeNull()
  })

  it('names the failure when core refuses the switch', async () => {
    const bridge = tauri({ agent_request: ({ body }) => { if (body.share_models !== undefined) throw new Error('This runtime is not in a fleet.'); return { share_models: false } } })
    render(RuntimesSection, { tauri: bridge, listen })
    const toggle = await screen.findByRole('switch', { name: 'Share model connections' })
    await waitFor(() => expect(toggle).toBeEnabled())
    await fireEvent.click(toggle)
    expect(await screen.findByText('This runtime is not in a fleet.')).toBeInTheDocument()
    // The switch shows core's answer, not the click.
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'false'))
  })

  it('takes shared connections back from MuniCloud when sharing turns off, and leaves the MuniCloud switch to Account', async () => {
    const state = { shareModels: true }
    const current = () => ({ ...runtimeList([local, studio]), shareViaCloud: true })
    const bridge = tauri({
      agent_request: fleet(state),
      runtime_list: current,
      runtime_refresh: current,
      runtime_share_models: () => ({ ...current(), shareViaCloud: false }),
    })
    render(RuntimesSection, { tauri: bridge, listen })
    const toggle = await screen.findByRole('switch', { name: 'Share model connections' })
    await waitFor(() => expect(toggle).toHaveAttribute('aria-checked', 'true'))
    expect(screen.queryByRole('switch', { name: /MuniCloud/ })).toBeNull()
    await fireEvent.click(toggle)
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith('runtime_share_models', { cloud: false }))
    expect(state.shareModels).toBe(false)
  })
})
