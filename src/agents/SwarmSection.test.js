import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import SwarmSection from './SwarmSection.svelte'
afterEach(cleanup)

const list = { runtimes: [
  { id: 'local', name: 'This computer', kind: 'local', local: true, status: 'online', admin: true },
  { id: 'box', name: 'Build box', kind: 'container', status: 'online', admin: true },
] }
const member = (runtime_id, runtime_name, extra = {}) => ({ runtime_id, runtime_name, form_factor: 'server', online: true, links: [], self: false, link: null, ...extra })

it('links two machines it manages with no link to copy, then unlinks a member', async () => {
  let linked = false
  const requests = []
  const invoke = vi.fn(async (_command, { runtime, operation, body }) => {
    requests.push({ runtime, operation, body })
    if (operation === 'swarm.invite') return { link: 'muniment://pair?v=1&id=core-box', expires_at: '', runtime_id: 'core-local' }
    if (operation === 'swarm.join') { linked = true; return { member: member('core-local', 'Laptop') } }
    if (operation === 'swarm.leave') { linked = false; return { removed: true } }
    if (operation !== 'swarm.members') return {}
    const self = runtime === 'local' ? 'core-local' : 'core-box'
    const other = runtime === 'local' ? 'core-box' : 'core-local'
    return { self, absence: { paused: false }, members: [member(self, runtime === 'local' ? 'Laptop' : 'Build box', { self: true }), ...(linked ? [member(other, runtime === 'local' ? 'Build box' : 'Laptop', { link: { direction: runtime === 'local' ? 'inbound' : 'outbound', state: 'connected', last_seen_at: new Date().toISOString() } })] : [])] }
  })
  render(SwarmSection, { tauri: { invoke }, list, listen: null })
  expect(await screen.findByText('No other machines in the fleet.')).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', { name: 'Join fleet' }))
  await waitFor(() => expect(screen.getByRole('status')).toHaveTextContent('Joined This computer and Build box.'))
  expect(requests.filter((request) => request.operation !== 'swarm.members' && !request.operation.startsWith('swarm.agents') && request.operation !== 'group.list' && request.operation !== 'agent.approvals')).toEqual([
    { runtime: 'local', operation: 'swarm.invite', body: {} },
    { runtime: 'box', operation: 'swarm.join', body: { link: 'muniment://pair?v=1&id=core-box' } },
  ])
  const laptop = await screen.findByRole('group', { name: 'Fleet member This computer' })
  expect(laptop).toHaveTextContent('With Build box')
  await fireEvent.click(within(laptop).getByRole('button', { name: 'Leave fleet' }))
  await waitFor(() => expect(requests.some((request) => request.operation === 'swarm.leave' && request.runtime === 'local' && request.body.runtime_id === 'core-box')).toBe(true))
})

it('shows a member it does not manage with the link state its own machines report', async () => {
  const invoke = vi.fn(async (_command, { operation }) => operation === 'swarm.members'
    ? { self: 'core-local', absence: { paused: false }, members: [member('core-local', 'Laptop', { self: true }), member('core-studio', 'Studio', { online: false, link: { direction: 'outbound', state: 'backoff', last_error: 'The machine did not answer.' } })] }
    : {})
  render(SwarmSection, { tauri: { invoke }, list: { runtimes: [list.runtimes[0]] }, listen: null })
  const studio = await screen.findByRole('group', { name: 'Fleet member Studio' })
  expect(studio).toHaveTextContent('Offline')
  expect(studio).toHaveTextContent('Retrying: The machine did not answer.')
  expect(within(studio).getByRole('button', { name: 'Remove from fleet' })).toBeInTheDocument()
  expect(screen.getByText('Pair a machine with admin access to add it.')).toBeInTheDocument()
})
