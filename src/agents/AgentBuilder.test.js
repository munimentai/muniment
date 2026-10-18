import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import AgentBuilder from './AgentBuilder.svelte'
import { draftAgent, draftForm, requestDraft } from './agent-draft.js'
afterEach(cleanup)

const draft = {
  name: 'Inbox brief',
  label: 'Morning briefer',
  instructions: 'Summarize the new email in my inbox and list what needs a reply.',
  skills: ['pack:skills/brief/SKILL.md'],
  tools_off: ['bash'],
  permission_mode: 'manual',
  routines: [{ id: 'extract-1', name: 'Summarize the new email in my inbox', enabled: true, trigger: { kind: 'cron', expr: '0 9 * * 1-5', tz: 'America/New_York' }, prompt: 'Summarize the new email in my inbox.', created_from: 'instructions' }],
}
const catalog = { builtin: [{ id: 'read', label: 'Read files', read_only: true }, { id: 'bash', label: 'Shell commands', read_only: false }], mcp: [], skills: [{ id: 'pack:skills/brief/SKILL.md', name: 'brief', description: 'Write a short briefing.' }] }
const modes = { modes: [{ id: 'manual', title: 'Ask before changes', available: true }, { id: 'bypass', title: 'Bypass permissions', available: true }], default_thread_mode: 'bypass', default_agent_mode: 'bypass' }

// A runtime that drafts like core: the first request answers an id, the
// next ask answers the draft. `answers` holds each finished draft in turn.
function runtime(answers = [{ draft, question: null, via: 'router' }]) {
  const requests = []
  let finished = 0
  const invoke = vi.fn(async (_command, { operation, body }) => {
    requests.push({ operation, body })
    if (operation === 'agent.draft') {
      if (!body.draft_id) return { draft_id: `draft-${requests.length}`, status: 'drafting' }
      return { draft_id: body.draft_id, status: 'done', ...answers[Math.min(finished++, answers.length - 1)] }
    }
    if (operation === 'agent.tool_catalog') return catalog
    if (operation === 'permission.modes') return modes
    if (operation === 'agent.save') return { agent: { ...body.agent, id: body.agent.id || 'agent-1', avatar: { style: 'muniment', seed: 'seed' } } }
    if (operation === 'agent.run_now') return { run: { id: 'run-1', agent_id: body.id, status: 'queued' } }
    throw new Error(`unexpected ${operation}`)
  })
  // Model round trips: each started draft is one model call in core.
  const started = () => requests.filter((request) => request.operation === 'agent.draft' && !request.body.draft_id)
  return { tauri: { invoke }, requests, started }
}

it('fills the form from one sentence with one model round trip, then saves with the trigger on', async () => {
  const { tauri, requests, started } = runtime()
  const onsaved = vi.fn()
  render(AgentBuilder, { tauri, onsaved, onrefine: vi.fn() })
  const describe = screen.getByRole('textbox', { name: 'Describe your agent' })
  await fireEvent.input(describe, { target: { value: 'Every weekday at 9am, summarize my inbox and list what needs a reply.' } })
  await fireEvent.keyDown(describe, { key: 'Enter' })
  const form = await screen.findByRole('form', { name: 'Agent draft' })
  // The budget: one draft started before the form shows, no chat run.
  expect(started()).toEqual([{ operation: 'agent.draft', body: { description: 'Every weekday at 9am, summarize my inbox and list what needs a reply.' } }])
  expect(requests.some((request) => request.operation === 'run.submit')).toBe(false)
  expect(within(form).getByRole('textbox', { name: 'Name' })).toHaveValue('Inbox brief')
  expect(within(form).getByRole('textbox', { name: 'Job title' })).toHaveValue('Morning briefer')
  expect(within(form).getByRole('textbox', { name: 'Instructions' })).toHaveValue(draft.instructions)
  expect(within(form).getByRole('switch', { name: 'Run Summarize the new email in my inbox' })).toHaveAttribute('aria-checked', 'true')
  expect(within(form).getByText('Weekdays at 09:00 · America/New_York')).toBeInTheDocument()
  await waitFor(() => expect(within(form).getByRole('button', { name: 'Shell commands' })).toHaveAttribute('aria-pressed', 'false'))
  expect(within(form).getByRole('button', { name: 'Read files' })).toHaveAttribute('aria-pressed', 'true')
  expect(within(form).getByRole('button', { name: 'brief' })).toHaveAttribute('aria-pressed', 'true')
  expect(within(form).getByRole('combobox', { name: 'Permission mode' })).toHaveValue('manual')
  await fireEvent.input(within(form).getByRole('textbox', { name: 'Name' }), { target: { value: 'Inbox' } })
  await fireEvent.click(within(form).getByRole('button', { name: 'Save' }))
  await waitFor(() => expect(onsaved).toHaveBeenCalled())
  const saved = requests.find((request) => request.operation === 'agent.save').body.agent
  expect(saved).toMatchObject({ id: '', name: 'Inbox', label: 'Morning briefer', schedule: null, skills: draft.skills, tools: { disabled: ['bash'] }, permissions: { mode: 'manual' } })
  expect(saved.routines).toEqual([expect.objectContaining({ id: 'extract-1', enabled: true, proposed: false })])
  expect(onsaved.mock.calls[0][0]).toMatchObject({ id: 'agent-1', runtimeId: 'local' })
  expect(started()).toHaveLength(1)
})

it('asks one question only when core asks, and drafts again with the answer', async () => {
  const question = { text: 'What should the agent do?', options: ['Research', 'Write'] }
  const { tauri, started } = runtime([{ draft: { ...draft, routines: [] }, question, via: 'router' }, { draft, question: null, via: 'router' }])
  render(AgentBuilder, { tauri, description: 'Make me an agent', autostart: true })
  const asked = await screen.findByRole('group', { name: 'Question' })
  expect(asked).toHaveTextContent('What should the agent do?')
  // The draft shows beside the question, so the question never blocks review.
  expect(screen.getByRole('form', { name: 'Agent draft' })).toBeInTheDocument()
  expect(screen.getByText('Runs when you ask.')).toBeInTheDocument()
  await fireEvent.click(within(asked).getByRole('button', { name: 'Research' }))
  await waitFor(() => expect(screen.queryByRole('group', { name: 'Question' })).toBeNull())
  await screen.findByRole('switch', { name: 'Run Summarize the new email in my inbox' })
  expect(started().map((request) => request.body)).toEqual([
    { description: 'Make me an agent' },
    { description: 'Make me an agent', answers: [{ question: 'What should the agent do?', answer: 'Research' }] },
  ])
})

it('runs a test with the triggers off, keeps the agent, and opens its chat', async () => {
  const { tauri, requests } = runtime()
  const onopenchat = vi.fn()
  const onsaved = vi.fn()
  render(AgentBuilder, { tauri, description: 'Summarize my inbox every weekday at 9am.', autostart: true, onopenchat, onsaved })
  const form = await screen.findByRole('form', { name: 'Agent draft' })
  await fireEvent.click(within(form).getByRole('button', { name: 'Run test now' }))
  const status = await within(form).findByRole('status')
  expect(status).toHaveTextContent('Test run started.')
  const test = requests.find((request) => request.operation === 'agent.save').body.agent
  expect(test.routines.every((routine) => routine.enabled === false)).toBe(true)
  expect(requests.find((request) => request.operation === 'agent.run_now').body).toEqual({ id: 'agent-1' })
  expect(onsaved).not.toHaveBeenCalled()
  await fireEvent.click(within(status).getByRole('button', { name: 'Open chat' }))
  expect(onopenchat.mock.calls[0][0]).toMatchObject({ id: 'agent-1', runtimeId: 'local' })
  // Save after the test keeps the agent's id and avatar and turns the trigger on.
  await fireEvent.click(within(form).getByRole('button', { name: 'Save' }))
  await waitFor(() => expect(onsaved).toHaveBeenCalled())
  const final = requests.filter((request) => request.operation === 'agent.save').at(-1).body.agent
  expect(final).toMatchObject({ id: 'agent-1', avatar: { seed: 'seed' } })
  expect(final.routines[0].enabled).toBe(true)
})

it('drafts from a thread and hands the words to the chat on Refine in chat', async () => {
  const { tauri, started } = runtime()
  const onrefine = vi.fn()
  render(AgentBuilder, { tauri, threadId: 'thread-9', threadTitle: 'Release notes', autostart: true, onrefine })
  expect(screen.getByText('Release notes')).toBeInTheDocument()
  const form = await screen.findByRole('form', { name: 'Agent draft' })
  expect(started().map((request) => request.body)).toEqual([{ thread_id: 'thread-9' }])
  await fireEvent.click(within(form).getByRole('button', { name: 'Refine in chat' }))
  expect(onrefine).toHaveBeenCalledWith(draft.instructions)
})

it('shows the refusal and keeps the sentence when a draft fails', async () => {
  const invoke = vi.fn(async (_command, { operation }) => {
    if (operation === 'agent.draft') throw 'Choose a Home folder before managing agents.'
    return operation === 'agent.tool_catalog' ? catalog : modes
  })
  render(AgentBuilder, { tauri: { invoke }, description: 'Watch my builds', autostart: true })
  expect(await screen.findByRole('alert')).toHaveTextContent('Choose a Home folder before managing agents.')
  expect(screen.getByRole('textbox', { name: 'Describe your agent' })).toHaveValue('Watch my builds')
  expect(screen.queryByRole('form', { name: 'Agent draft' })).toBeNull()
})

it('asks after a draft until it is done and stops when the builder closes', async () => {
  let asks = 0
  const invoke = vi.fn(async (_command, { body }) => body.draft_id ? (++asks < 3 ? { draft_id: 'd', status: 'drafting' } : { draft_id: 'd', status: 'done', draft, question: null, via: 'pi' }) : { draft_id: 'd', status: 'drafting' })
  const wait = vi.fn(async () => {})
  const answer = await requestDraft({ invoke }, 'local', { description: 'x' }, { wait })
  expect(answer).toEqual({ draft, question: null, via: 'pi' })
  expect(wait).toHaveBeenCalledTimes(3)
  expect(invoke.mock.calls.filter(([, args]) => !args.body.draft_id)).toHaveLength(1)
  expect(await requestDraft({ invoke }, 'local', { description: 'x' }, { wait, cancelled: () => true })).toBeNull()
  let clock = 0
  await expect(requestDraft({ invoke: async () => ({ draft_id: 'd', status: 'drafting' }) }, 'local', { description: 'x' }, { wait, now: () => (clock += 60_000) })).rejects.toThrow('The draft took too long.')
})

it('builds the save body from the form', () => {
  const form = draftForm(draft)
  form.routines[0].enabled = false
  form.mode = ''
  expect(draftAgent(form)).toEqual({ id: '', project_id: null, name: 'Inbox brief', label: 'Morning briefer', instructions: draft.instructions, schedule: null, skills: draft.skills, tools: { disabled: ['bash'] }, routines: [{ ...draft.routines[0], enabled: false, proposed: false }] })
})
