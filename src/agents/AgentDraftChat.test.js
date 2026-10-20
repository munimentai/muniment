import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import AgentDraftChat from './AgentDraftChat.svelte'
import { draftAgent, draftForm, requestDraft } from './agent-draft.js'
afterEach(cleanup)

const draft = {
  name: 'Inbox brief',
  label: 'Morning briefer',
  instructions: 'Summarize the new email in my inbox and list what needs a reply.',
  skills: ['pack:skills/brief/SKILL.md'],
  permission_mode: 'manual',
  routines: [{ id: 'extract-1', name: 'Summarize the new email in my inbox', enabled: true, trigger: { kind: 'cron', expr: '0 9 * * 1-5', tz: 'America/New_York' }, prompt: 'Summarize the new email in my inbox.', created_from: 'instructions' }],
}
const catalog = { builtin: [{ id: 'read', label: 'Read files', read_only: true }, { id: 'bash', label: 'Shell commands', read_only: false }], mcp: [], skills: [{ id: 'pack:skills/brief/SKILL.md', name: 'brief', description: 'Write a short briefing.' }] }
const turn = (message, reply = { draft, question: null, via: 'router' }) => ({ message, at: '2026-10-19T10:00:00Z', reply })

function runtime() {
  const requests = []
  const invoke = vi.fn(async (_command, { operation, body }) => {
    requests.push({ operation, body })
    if (operation === 'agent.tool_catalog') return catalog
    if (operation === 'agent.read') return { agent: { ...draft, id: body.id, avatar: { style: 'muniment', seed: 'seed' }, model: { provider: 'openai', id: 'gpt-6' } } }
    if (operation === 'agent.save') return { agent: { ...body.agent, id: body.agent.id || 'agent-1', avatar: body.agent.avatar ?? { style: 'muniment', seed: 'seed' } } }
    if (operation === 'agent.run_now') return { run: { id: 'run-1', agent_id: body.id, status: 'queued' } }
    throw new Error(`unexpected ${operation}`)
  })
  return { tauri: { invoke }, requests }
}

it('shows each message and the draft core answered it with, the newest with Save and Test run', async () => {
  const { tauri } = runtime()
  render(AgentDraftChat, { tauri, model: 'GPT-6 · default', turns: [turn('Every weekday at 9am, summarize my inbox'), turn('Add a list of replies', { draft: { ...draft, name: 'Inbox replies' }, question: null, via: 'router' })] })
  expect(screen.getByText('Every weekday at 9am, summarize my inbox')).toBeInTheDocument()
  const drafts = screen.getAllByRole('article')
  expect(drafts).toHaveLength(2)
  const newest = drafts[1]
  expect(newest).toHaveAccessibleName('Draft: Inbox replies')
  expect(within(newest).getByText(draft.instructions)).toBeInTheDocument()
  expect(within(newest).getByText('Weekdays at 09:00 · America/New_York')).toBeInTheDocument()
  await waitFor(() => expect(within(newest).getByText('brief')).toBeInTheDocument())
  expect(within(newest).getByText('GPT-6 · default')).toBeInTheDocument()
  expect(within(newest).getByRole('button', { name: 'Save' })).toBeInTheDocument()
  expect(within(newest).getByRole('button', { name: 'Test run' })).toBeInTheDocument()
  expect(within(drafts[0]).queryByRole('button', { name: 'Save' })).toBeNull()
})

it('shows the message a draft is answering and blocks the actions until it lands', () => {
  const { tauri } = runtime()
  render(AgentDraftChat, { tauri, turns: [turn('Summarize my inbox')], pending: 'Make it 8am' })
  expect(screen.getByText('Make it 8am')).toBeInTheDocument()
  expect(screen.getByRole('status')).toHaveTextContent('Drafting the agent…')
  expect(screen.getByRole('button', { name: 'Save' })).toBeDisabled()
})

it('sends an option of a question as the next message', async () => {
  const { tauri } = runtime()
  const onsend = vi.fn()
  render(AgentDraftChat, { tauri, onsend, turns: [turn('Make me an agent', { draft: { ...draft, routines: [] }, question: { text: 'What should the agent do?', options: ['Research', 'Write'] }, via: 'router' })] })
  const asked = screen.getByRole('group', { name: 'Question' })
  expect(asked).toHaveTextContent('What should the agent do?')
  expect(screen.getByText('Runs when you ask.')).toBeInTheDocument()
  await fireEvent.click(within(asked).getByRole('button', { name: 'Research' }))
  expect(onsend).toHaveBeenCalledWith('Research')
})

it('runs a test with the triggers off, then saves the same agent with the triggers on', async () => {
  const { tauri, requests } = runtime()
  const onopenchat = vi.fn()
  const onsaved = vi.fn()
  const { rerender } = render(AgentDraftChat, { tauri, projectId: 'p1', turns: [turn('Summarize my inbox every weekday at 9am.')], onopenchat, onsaved })
  await fireEvent.click(screen.getByRole('button', { name: 'Test run' }))
  const status = await screen.findByRole('status')
  expect(status).toHaveTextContent('Test run started.')
  const test = requests.find((request) => request.operation === 'agent.save').body.agent
  expect(test).toMatchObject({ id: '', projectId: 'p1', name: 'Inbox brief' })
  expect(test.routines.every((routine) => routine.enabled === false)).toBe(true)
  expect(requests.find((request) => request.operation === 'agent.run_now').body).toEqual({ id: 'agent-1', test: true })
  expect(onsaved.mock.calls[0][1]).toEqual({ test: true })
  await fireEvent.click(within(status).getByRole('button', { name: 'Open chat' }))
  expect(onopenchat.mock.calls[0][0]).toMatchObject({ id: 'agent-1', runtimeId: 'local' })
  await rerender({ runs: [{ id: 'run-1', agent_id: 'agent-1', runtimeId: 'local', status: 'completed', thread_id: 'thread-run' }] })
  expect(screen.getByRole('status')).toHaveTextContent('Test run finished.')
  await fireEvent.click(screen.getByRole('button', { name: 'Open result' }))
  expect(onopenchat.mock.calls[1][1]).toMatchObject({ id: 'run-1', thread_id: 'thread-run' })
  await fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
  await waitFor(() => expect(onsaved).toHaveBeenCalledTimes(2))
  const final = requests.filter((request) => request.operation === 'agent.save').at(-1).body.agent
  expect(final).toMatchObject({ id: 'agent-1', avatar: { seed: 'seed' } })
  expect(final.routines[0].enabled).toBe(true)
})

it('updates the agent a reopened chat saved before and keeps its model', async () => {
  const { tauri, requests } = runtime()
  render(AgentDraftChat, { tauri, agentId: 'agent-7', turns: [turn('Summarize my inbox')] })
  await fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
  await waitFor(() => expect(requests.some((request) => request.operation === 'agent.save')).toBe(true))
  expect(requests.find((request) => request.operation === 'agent.read').body).toEqual({ id: 'agent-7' })
  const saved = requests.find((request) => request.operation === 'agent.save').body.agent
  expect(saved).toMatchObject({ id: 'agent-7', model: { provider: 'openai', id: 'gpt-6' }, name: 'Inbox brief' })
  expect(await screen.findByRole('status')).toHaveTextContent('Saved.')
})

it('asks core with a chat message and polls until the draft is done', async () => {
  let asks = 0
  const creation = { threadId: 'c1', kind: 'agent', turns: [turn('x')] }
  const invoke = vi.fn(async (_command, { body }) => body.draft_id ? (++asks < 3 ? { draft_id: 'd', status: 'drafting' } : { draft_id: 'd', status: 'done', draft, question: null, via: 'pi', creation }) : { draft_id: 'd', status: 'drafting' })
  const wait = vi.fn(async () => {})
  const answer = await requestDraft({ invoke }, 'local', { chatId: 'c1', message: 'x', threadId: 't9' }, { wait })
  expect(answer).toEqual({ draft, question: null, via: 'pi', creation })
  expect(invoke.mock.calls[0][1].body).toEqual({ chat_id: 'c1', message: 'x', thread_id: 't9' })
  expect(wait).toHaveBeenCalledTimes(3)
  expect(await requestDraft({ invoke }, 'local', { chatId: 'c1', message: 'x' }, { wait, cancelled: () => true })).toBeNull()
  let clock = 0
  await expect(requestDraft({ invoke: async () => ({ draft_id: 'd', status: 'drafting' }) }, 'local', { chatId: 'c1', message: 'x' }, { wait, now: () => (clock += 60_000) })).rejects.toThrow('The draft took too long.')
})

it('builds the save body from the draft', () => {
  const form = draftForm(draft)
  form.routines[0].enabled = false
  form.mode = ''
  expect(draftAgent(form)).toEqual({ id: '', projectId: null, name: 'Inbox brief', label: 'Morning briefer', instructions: draft.instructions, schedule: null, skills: draft.skills, routines: [{ ...draft.routines[0], enabled: false, proposed: false }] })
})
