import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import SyncStatus from './SyncStatus.svelte'
import QueuedNotice from './QueuedNotice.svelte'
import OfflineSettings from './OfflineSettings.svelte'
import AuditLog from './AuditLog.svelte'
import MachinePicker from './MachinePicker.svelte'
import OwnerChip from './OwnerChip.svelte'

afterEach(cleanup)

const swarm = { self: { local: 'core-laptop' }, members: [{ runtime_id: 'core-laptop', runtime_name: 'Laptop', online: true }, { runtime_id: 'core-studio', runtime_name: 'Studio', online: false }] }

function agentBridge(answers) {
  const invoke = vi.fn(async (command, payload) => {
    if (command === 'project_list') return { projects: { mine: 'Taxes' }, remote: { p1: { name: 'Launch', ownerRuntime: 'core-studio' } }, threads: {} }
    if (command !== 'agent_request') throw new Error(`unexpected command: ${command}`)
    const answer = answers[payload.operation]
    if (answer instanceof Error) throw answer
    return typeof answer === 'function' ? answer(payload.body) : answer
  })
  return { invoke }
}

describe('the sync line', () => {
  it('shows Syncing N, opens the bootstrap panel on its own and names where each sign-in comes from', async () => {
    const status = { state: 'bootstrapping', classes: [{ class: 'agent', held: 6, total: 6, percent: 100 }, { class: 'history', held: 37, total: 120, percent: 30 }] }
    const credentials = { accounts: [{ name: 'router:work', label: 'Work', holder: 'core-laptop', holder_name: 'Laptop', this_machine: false, usable_here: false }, { name: 'pi:anthropic', holder: 'core-studio', holder_name: 'Studio', this_machine: false, usable_here: false }] }
    render(SyncStatus, { status, ops: [], credentials, swarm })
    const line = screen.getByRole('button', { name: /Syncing 1/ })
    expect(line).toHaveAttribute('aria-expanded', 'true')
    const panel = screen.getByRole('region', { name: 'Sync progress' })
    expect(within(panel).getByRole('progressbar', { name: 'History' })).toHaveAttribute('aria-valuenow', '30')
    expect(within(panel).getByText('arriving from Laptop')).toBeInTheDocument()
    expect(within(panel).getByText('waiting for a holder')).toBeInTheDocument()
    await fireEvent.click(line)
    expect(screen.queryByRole('region', { name: 'Sync progress' })).toBeNull()
  })

  it('shows nothing for a machine alone and Synced once caught up', () => {
    const { container } = render(SyncStatus, { status: { state: 'alone' } })
    expect(container.textContent.trim()).toBe('')
    cleanup()
    render(SyncStatus, { status: { state: 'synced', classes: [] }, ops: [] })
    expect(screen.getByRole('button', { name: /^Synced/ })).toHaveAttribute('aria-expanded', 'false')
  })
})

describe('the queued composer', () => {
  it('names the offline owner and cancels the queued message', async () => {
    const oncancel = vi.fn(async () => {})
    const op = { op_id: 'o1', state: 'queued', owner_name: 'Studio', reason: { code: 'owner_offline' } }
    render(QueuedNotice, { op, oncancel })
    expect(screen.getByText("Queued. Studio is offline. Sends when it's back.")).toBeInTheDocument()
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(oncancel).toHaveBeenCalledWith(op)
  })

  it('offers Move here for an agent whose machine is away', async () => {
    const onmove = vi.fn(async () => {})
    render(QueuedNotice, { moveSuggested: true, onmove })
    await fireEvent.click(screen.getByRole('button', { name: 'Move here' }))
    expect(onmove).toHaveBeenCalled()
  })
})

describe('available offline', () => {
  it('saves each choice through core', async () => {
    const offline = { thread_days: 30, attachments: 'open' }
    const tauri = agentBridge({
      'fleet.sync_status': { state: 'synced', classes: [], offline },
      'fleet.pins': { pins: [], offline: { artifacts: 'pinned' }, usage: {} },
      'fleet.offline_set': (body) => ({ offline: { ...offline, ...body } }),
      'fleet.pin_set': (body) => ({ pins: body.pins ?? [], offline: { artifacts: body.offline?.artifacts ?? 'pinned' }, usage: {} }),
    })
    render(OfflineSettings, { tauri })
    const threads = await screen.findByRole('radiogroup', { name: 'Threads' })
    await fireEvent.click(within(threads).getByRole('radio', { name: 'All' }))
    expect(tauri.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'fleet.offline_set', body: { thread_days: 0 } })
    await waitFor(() => expect(within(threads).getByRole('radio', { name: 'All' })).toHaveAttribute('aria-checked', 'true'))
    await fireEvent.click(within(screen.getByRole('radiogroup', { name: 'Artifacts' })).getByRole('radio', { name: 'All' }))
    expect(tauri.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'fleet.pin_set', body: { offline: { artifacts: 'all' } } })
    await fireEvent.click(within(screen.getByRole('radiogroup', { name: 'Projects' })).getByRole('radio', { name: 'Chosen' }))
    await fireEvent.click(await screen.findByRole('checkbox', { name: 'Launch' }))
    expect(tauri.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'local', operation: 'fleet.pin_set', body: { pins: [{ class: 'project', key: 'p1', pinned: true }] } })
  })

  it('stays hidden outside a fleet', async () => {
    const tauri = agentBridge({ 'fleet.sync_status': { state: 'alone', classes: [] }, 'fleet.pins': new Error('no swarm') })
    render(OfflineSettings, { tauri })
    await waitFor(() => expect(tauri.invoke).toHaveBeenCalledWith('agent_request', expect.objectContaining({ operation: 'fleet.sync_status' })))
    expect(screen.queryByText('Available offline')).toBeNull()
  })
})

describe('the fleet log', () => {
  it('lists each entry with its machines, and hides when the runtime refuses', async () => {
    const tauri = agentBridge({
      'fleet.audit': { entries: [{ id: 'e1', at: '2026-01-01T14:00:00Z', kind: 'role_changed', actor_runtime: 'core-laptop', runtime_id: 'core-studio', record: { from: 'full', to: 'worker' } }], more: false },
      'swarm.members': swarm,
    })
    render(AuditLog, { tauri })
    const log = await screen.findByRole('list', { name: 'Fleet log' })
    expect(log).toHaveTextContent('Changed a role')
    expect(log).toHaveTextContent('Laptop → Studio')
    expect(log).toHaveTextContent('full to worker')
    cleanup()
    render(AuditLog, { tauri: agentBridge({ 'fleet.audit': new Error('unknown'), 'swarm.members': swarm }) })
    await new Promise((resolve) => setTimeout(resolve, 0))
    expect(screen.queryByText('Log')).toBeNull()
  })
})

describe('the machine picker', () => {
  const list = { selected: 'local', runtimes: [{ id: 'local', name: 'This computer', kind: 'local', local: true, active: true, status: 'online' }] }

  it('lists fleet members beside this computer and picks one for a new thread', async () => {
    const onoffload = vi.fn()
    const pod = { coreId: 'core-pod', name: 'Pod', kind: 'container', online: true }
    const { rerender } = render(MachinePicker, { list, fleet: [pod], onoffload })
    await fireEvent.click(screen.getByRole('button', { name: /Machine: This computer/ }))
    await fireEvent.click(await screen.findByRole('menuitemradio', { name: 'Pod' }))
    expect(onoffload).toHaveBeenCalledWith(pod)
    await rerender({ list, fleet: [pod], offload: pod, onoffload })
    expect(screen.getByRole('button', { name: /Machine: Pod/ })).toBeInTheDocument()
  })

  it('names another member that owns the open thread', () => {
    render(MachinePicker, { list, locked: 'local', owner: { name: 'Studio', kind: 'desktop', online: false } })
    const chip = screen.getByRole('note', { name: 'This thread stays on Studio' })
    expect(chip).toHaveTextContent('on Studio')
    expect(within(chip).getByRole('img', { name: 'Offline' })).toBeInTheDocument()
  })

  it('draws an owner chip with its offline mark', () => {
    render(OwnerChip, { machine: { name: 'Studio', kind: 'desktop', online: false } })
    expect(screen.getByText('on Studio')).toBeInTheDocument()
    expect(screen.getByText(', offline')).toBeInTheDocument()
  })
})
