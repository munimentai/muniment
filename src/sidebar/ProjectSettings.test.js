import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ProjectSettings from './ProjectSettings.svelte'
afterEach(cleanup)

const project = { id: 'p1', name: 'Homelab', kind: 'project', agent_ids: ['scout'] }
const agent = { id: 'scout', name: 'Scout', runtimeId: 'local', routines: [{ id: 'daily', name: 'Daily brief', enabled: true, trigger: { kind: 'cron', expr: '0 9 * * *', tz: 'UTC' } }] }

it('opens a run from a trigger card\'s History and retries a failed one', async () => {
  const at = new Date().toISOString()
  const failed = { id: 'r1', agent_id: 'scout', thread_id: 't1', status: 'failed', trigger: { kind: 'cron', routine_id: 'daily' }, created_at: at, updated_at: at }
  const queued = { id: 'r2', agent_id: 'scout', thread_id: null, status: 'queued', created_at: at, updated_at: at }
  const requests = []
  const invoke = vi.fn(async (command, args) => {
    if (command === 'project_settings') return { instructions: '' }
    if (command !== 'agent_request') return null
    requests.push(args)
    if (args.operation === 'agent.runs') return { runs: [failed], next_cursor: null }
    if (args.operation === 'agent.replays') return { replays: [{ replay_id: 'r1', agent_id: 'scout', thread_id: 't1', error: 'boom', failed_at: at }] }
    if (args.operation === 'agent.replay') return { run: queued, replay: { replay_id: 'r1', replayed_by: ['r2'] } }
    return {}
  })
  const onopenrun = vi.fn()
  const onrun = vi.fn()
  render(ProjectSettings, { tauri: { invoke }, project, agents: [agent], onback: vi.fn(), onnewthread: vi.fn(), onaddtrigger: vi.fn(), onopenrun, onrun })
  await fireEvent.click(await screen.findByRole('tab', { name: 'History' }))
  const history = await screen.findByRole('region', { name: 'History' })
  const row = await within(history).findByRole('button', { name: /^Daily brief, Today at / })
  expect(row).toBeEnabled()
  await fireEvent.click(row)
  expect(onopenrun).toHaveBeenCalledWith(expect.objectContaining({ id: 'r1', thread_id: 't1' }))
  await fireEvent.click(await within(history).findByRole('button', { name: 'Retry Daily brief' }))
  await waitFor(() => expect(requests).toContainEqual({ runtime: 'local', operation: 'agent.replay', body: { id: 'scout', replay_id: 'r1' } }))
  expect(onrun).toHaveBeenCalledWith({ ...queued, runtimeId: 'local' })
})
