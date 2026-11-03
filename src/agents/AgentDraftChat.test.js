import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import AgentDraftChat from './AgentDraftChat.svelte'
import { draftAgent, draftForm, requestDraft, sameAgent } from './agent-draft.js'
import { saveTimeSetting } from '../lib/time-format.svelte.js'
afterEach(cleanup)
// Times read in one zone on a 24-hour clock, whatever the computer uses.
beforeEach(() => saveTimeSetting(null, { zone: 'America/New_York', clock: '24h' }))
afterEach(() => saveTimeSetting(null, { zone: '', clock: '' }))

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
  // The draft names no time zone.
  expect(within(newest).getByText('Weekdays at 09:00')).toBeInTheDocument()
  expect(newest).not.toHaveTextContent('America/New_York')
  await waitFor(() => expect(within(newest).getByText('brief')).toBeInTheDocument())
  expect(within(newest).getByText('GPT-6 · default')).toBeInTheDocument()
  expect(within(newest).getByRole('button', { name: 'Save' })).toBeInTheDocument()
  expect(within(newest).getByRole('button', { name: 'Test run' })).toBeInTheDocument()
  expect(within(drafts[0]).queryByRole('button', { name: 'Save' })).toBeNull()
})

it('says Muniment picked a time the message did not name, and Save keeps the draft unchanged', async () => {
  const { tauri } = runtime()
  const picked = { ...draft, routines: [{ ...draft.routines[0], trigger: { kind: 'cron', expr: '0 9 * * 5', tz: 'America/New_York' }, default_time: true }] }
  render(AgentDraftChat, { tauri, turns: [turn('On Fridays, back up the notes folder', { draft: picked, question: null, via: 'router' })] })
  const shown = screen.getByRole('article', { name: 'Draft: Inbox brief' })
  const note = within(shown).getByRole('note')
  expect(note).toHaveTextContent('Muniment picked the time. Change it if needed.')
  // The note is its own line under the time, never run into it.
  expect(note.tagName).toBe('P')
  expect(note.previousSibling.textContent).toBe('Fridays at 09:00')
  cleanup()
  render(AgentDraftChat, { tauri, turns: [turn('Every weekday at 9am, summarize my inbox')] })
  expect(screen.queryByRole('note')).toBeNull()
  // The mark is core's, so a saved agent without it is the same agent.
  const saved = { ...draftAgent(draftForm(draft)), id: 'agent-7' }
  expect(sameAgent(draftAgent(draftForm({ ...draft, routines: [{ ...draft.routines[0], default_time: true }] }), { saved }), saved)).toBe(true)
  // A picked time saves its routine on, as a named time does, and a test run keeps both off.
  for (const test of [false, true]) {
    const both = draftAgent(draftForm({ ...draft, routines: [draft.routines[0], picked.routines[0]] }), { test })
    expect(both.routines.map((routine) => [routine.enabled, routine.proposed])).toEqual([[!test, false], [!test, false]])
  }
})

// The routines core answers for instructions that say "Every Friday at 4 PM,
// post the weekly greeting. On Fridays, back up the notes folder.": the draft
// holds the model's schedule, and the saved agent adds the backup as a
// proposal at a time Muniment picked.
const tz = 'America/New_York'
const greeting = { created_from: 'instructions', enabled: true, id: 'extract-a51b949e6527c243', last_fired_at: null, last_result: null, name: 'Post the weekly greeting', prompt: 'Post the weekly greeting.', trigger: { expr: '0 16 * * 5', kind: 'cron', tz } }
const backup = { created_from: 'instructions', default_time: true, enabled: false, id: 'extract-02d2ee9277b37b2a', last_fired_at: null, last_result: null, name: 'Back up the notes folder', prompt: 'Back up the notes folder.', proposed: true, trigger: { expr: '0 9 * * 5', kind: 'cron', tz } }
const weekly = { name: 'Weekly helper', label: 'Helper', instructions: 'Every Friday at 4 PM, post the weekly greeting. On Fridays, back up the notes folder.', skills: [], permission_mode: 'manual', routines: [greeting] }
const savedWeekly = (draft, routines) => ({ name: draft.name, label: draft.label, instructions: draft.instructions, skills: draft.skills, id: 'agent-9', permissions: { mode: 'manual' }, schedule: null, avatar: { style: 'muniment', seed: 'seed' }, routines })

it.each([
  ['Fridays at 4 PM with a proposal core added', [turn('Post the greeting on Fridays at 4 PM', { draft: weekly, question: null, via: 'router' })], [{ ...greeting, next_fire_at: 1793988000 }, backup]],
  ['Fridays at a picked time', [turn('On Fridays, back up the notes folder', { draft: { ...weekly, instructions: 'On Fridays, back up the notes folder.', routines: [{ ...backup, enabled: true, proposed: undefined }] }, question: null, via: 'router' })], [{ ...backup, enabled: true, proposed: undefined, default_time: undefined }]],
  // The backup proposal was accepted, and an earlier draft ran the greeting at its time.
  ['Fridays at 4 PM after an earlier draft at the accepted proposal\'s time', [turn('Post the greeting on Fridays', { draft: { ...weekly, routines: [{ ...greeting, id: 'extract-5e0c1d', trigger: { expr: '0 9 * * 5', kind: 'cron', tz }, default_time: true }] }, question: null, via: 'router' }), { ...turn('Make it 4 PM', { draft: weekly, question: null, via: 'router' }), at: '2026-10-19T10:05:00Z' }], [greeting, { ...backup, enabled: true, proposed: undefined, default_time: undefined }]],
])('rests Save changes on a reopened chat for an agent with routines: %s', async (_name, turns, routines) => {
  const stored = savedWeekly(turns.at(-1).reply.draft, routines)
  const requests = []
  const invoke = vi.fn(async (_command, { operation, body }) => {
    requests.push({ operation, body })
    if (operation === 'agent.tool_catalog') return catalog
    if (operation === 'agent.read') return { agent: stored, primary_thread_id: null }
    if (operation === 'agent.save') return { agent: { ...stored, ...body.agent } }
    throw new Error(`unexpected ${operation}`)
  })
  render(AgentDraftChat, { tauri: { invoke }, agentId: 'agent-9', turns })
  await waitFor(() => expect(requests.some((request) => request.operation === 'agent.read')).toBe(true))
  await waitFor(() => expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled())
  // A Save would send the agent's routines as they are saved.
  const earlier = turns.slice(0, -1).flatMap((entry) => entry.reply.draft.routines)
  const body = draftAgent(draftForm(turns.at(-1).reply.draft), { saved: stored, earlier })
  expect(body.routines.map((routine) => routine.id).sort()).toEqual(routines.map((routine) => routine.id).sort())
})

// The scheduler writes fire times, results, polls and run state on the
// saved agent after a Save. None of them is a change a person made.
it('rests Save changes on a reopened chat while the scheduler writes its routines after a Save', async () => {
  const turns = [turn('Post the greeting on Fridays at 4 PM', { draft: weekly, question: null, via: 'router' })]
  const scheduled = { ...greeting, next_fire_at: 1793988000, last_fired_at: '2026-11-03T09:00:00Z', last_result: { status: 'completed', run_id: 'run-4' }, last_poll: { at: '2026-11-03T09:00:00Z', fired: true, failures: 0 }, cut_short: false }
  const stored = { ...savedWeekly(weekly, [scheduled, { ...backup, next_fire_at: 1793990000 }]), schedule: { cron: '0 16 * * 5', enabled: true }, paused_reason: null, epoch: 3 }
  expect(sameAgent(draftAgent(draftForm(weekly), { saved: stored }), stored)).toBe(true)
  const invoke = vi.fn(async (_command, { operation }) => {
    if (operation === 'agent.tool_catalog') return catalog
    if (operation === 'agent.read') return { agent: stored, primary_thread_id: null }
    throw new Error(`unexpected ${operation}`)
  })
  render(AgentDraftChat, { tauri: { invoke }, agentId: 'agent-9', turns })
  await waitFor(() => expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled())
})

// The window keeps one builder while it moves from one agent chat to the
// next, so the builder reads the agent each chat names, never the last one.
it('reads the agent fresh when the chat names another agent', async () => {
  const other = { ...draft, name: 'Notes keeper', label: 'Keeper', instructions: 'Keep the notes folder tidy.', routines: [] }
  const agents = { 'agent-1': { ...draft, id: 'agent-1', permissions: { mode: 'manual' } }, 'agent-2': { ...other, id: 'agent-2', permissions: { mode: 'manual' } } }
  const reads = []
  const invoke = vi.fn(async (_command, { operation, body }) => {
    if (operation === 'agent.tool_catalog') return catalog
    if (operation === 'agent.read') { reads.push(body.id); return { agent: agents[body.id] } }
    throw new Error(`unexpected ${operation}`)
  })
  const { rerender } = render(AgentDraftChat, { tauri: { invoke }, agentId: 'agent-1', turns: [turn('Summarize my inbox')] })
  await waitFor(() => expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled())
  await rerender({ agentId: 'agent-2', turns: [turn('Keep my notes tidy', { draft: other, question: null, via: 'router' })] })
  await waitFor(() => expect(reads).toEqual(['agent-1', 'agent-2']))
  await waitFor(() => expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled())
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

it('rests Save changes on a reopened chat until its draft differs from the saved agent', async () => {
  const { tauri, requests } = runtime()
  // Core keeps the agent as the chat's newest draft saved it, with its own fields beside.
  const stored = { ...draftAgent(draftForm(draft)), id: 'agent-7', avatar: { style: 'muniment', seed: 'seed' }, model: { provider: 'openai', id: 'gpt-6' }, routines: draft.routines.map((routine) => ({ ...routine, trigger: { ...routine.trigger }, enabled: true, proposed: false, next_fire_at: '2026-10-20T13:00:00Z' })) }
  const read = tauri.invoke.getMockImplementation()
  tauri.invoke.mockImplementation(async (command, payload) => payload.operation === 'agent.read' ? { agent: stored } : read(command, payload))
  const first = [turn('Summarize my inbox')]
  const { rerender } = render(AgentDraftChat, { tauri, agentId: 'agent-7', turns: first })
  const save = screen.getByRole('button', { name: 'Save changes' })
  await waitFor(() => expect(save).toBeDisabled())
  expect(save).toHaveAttribute('data-variant', 'outline')
  expect(screen.getByRole('button', { name: 'Test run' })).toBeEnabled()
  expect(requests.some((request) => request.operation === 'agent.save')).toBe(false)
  // A message that drafts the same agent leaves Save at rest, and a change wakes it.
  await rerender({ turns: [...first, { ...turn('Keep it as is'), at: '2026-10-19T10:05:00Z' }] })
  expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled()
  await rerender({ turns: [...first, turn('Make it 8am', { draft: { ...draft, routines: [{ ...draft.routines[0], trigger: { ...draft.routines[0].trigger, expr: '0 8 * * 1-5' } }] }, question: null, via: 'router' })] })
  expect(screen.getByRole('button', { name: 'Save changes' })).toBeEnabled()
  await rerender({ turns: [...first, turn('Rename it', { draft: { ...draft, name: 'Inbox digest' }, question: null, via: 'router' })] })
  expect(screen.getByRole('button', { name: 'Save changes' })).toBeEnabled()
})

it('rests Save changes on a reopened chat against the agent exactly as core saved it, with its proposals and webhook id', async () => {
  const { tauri } = runtime()
  const webhook = { name: 'On push', enabled: true, trigger: { kind: 'webhook' }, prompt: 'Read the pushed commits.' }
  const drafted = { ...draft, routines: [...draft.routines, webhook] }
  // The answer `agent.read` gives for the agent this draft saved: core added
  // ids, the webhook's id and default sender, run state, and a routine it
  // proposed from the instructions, which waits off for the user.
  const stored = {
    id: 'agent-7', projectId: null, name: 'Inbox brief', label: 'Morning briefer', instructions: draft.instructions, schedule: null,
    skills: ['pack:skills/brief/SKILL.md'], permissions: { mode: 'manual' }, avatar: { style: 'muniment', seed: 'seed' }, model: { provider: 'openai', id: 'gpt-6' },
    routines: [
      { id: 'extract-1', name: 'Summarize the new email in my inbox', enabled: true, trigger: { kind: 'cron', expr: '0 9 * * 1-5', tz: 'America/New_York' }, prompt: 'Summarize the new email in my inbox.', created_from: 'instructions', last_fired_at: null, last_result: null, next_fire_at: 1760965200 },
      { id: '01930000-0000-7000-8000-000000000001', name: 'On push', enabled: true, trigger: { kind: 'webhook', hook_id: '01930000-0000-7000-8000-000000000002', preset: 'generic' }, prompt: 'Read the pushed commits.', created_from: 'user', last_fired_at: null, last_result: null },
      { id: 'extract-9f2c', name: 'List what needs a reply', enabled: false, trigger: { kind: 'cron', expr: '0 17 * * 5', tz: 'America/New_York' }, prompt: 'List what needs a reply.', created_from: 'instructions', last_fired_at: null, last_result: null, proposed: true },
    ],
  }
  const read = tauri.invoke.getMockImplementation()
  tauri.invoke.mockImplementation(async (command, payload) => payload.operation === 'agent.read' ? { agent: stored } : read(command, payload))
  render(AgentDraftChat, { tauri, agentId: 'agent-7', turns: [turn('Summarize my inbox', { draft: drafted, question: null, via: 'router' })] })
  await waitFor(() => expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled())
  expect(sameAgent(draftAgent(draftForm({ ...drafted, routines: [draft.routines[0], { ...webhook, trigger: { kind: 'webhook', preset: 'github' } }] }), { saved: stored }), stored)).toBe(false)
})

it('saves a draft again over a saved agent and keeps the routines core proposed and the user added', async () => {
  const { tauri, requests } = runtime()
  const proposal = { id: 'extract-9f2c', name: 'List what needs a reply', enabled: false, trigger: { kind: 'cron', expr: '0 17 * * 5', tz: 'America/New_York' }, prompt: 'List what needs a reply.', created_from: 'instructions', last_fired_at: null, last_result: null, proposed: true }
  const added = { id: '01930000-0000-7000-8000-000000000001', name: 'On push', enabled: true, trigger: { kind: 'webhook', hook_id: '01930000-0000-7000-8000-000000000002', preset: 'github' }, prompt: 'Read the pushed commits.', created_from: 'user', last_fired_at: null, last_result: null }
  const stored = { ...draftAgent(draftForm(draft)), id: 'agent-7', permissions: { mode: 'manual' }, avatar: { style: 'muniment', seed: 'seed' }, routines: [{ ...draft.routines[0], last_fired_at: null, last_result: null, next_fire_at: 1760965200 }, proposal, added] }
  const read = tauri.invoke.getMockImplementation()
  tauri.invoke.mockImplementation(async (command, payload) => payload.operation === 'agent.read' ? { agent: stored } : read(command, payload))
  const first = turn('Summarize my inbox')
  const { rerender } = render(AgentDraftChat, { tauri, agentId: 'agent-7', turns: [first] })
  await waitFor(() => expect(screen.getByRole('button', { name: 'Save changes' })).toBeDisabled())
  // A rename keeps every routine.
  await rerender({ turns: [first, { ...turn('Call it Inbox digest', { draft: { ...draft, name: 'Inbox digest' }, question: null, via: 'router' }), at: '2026-10-19T10:05:00Z' }] })
  await fireEvent.click(screen.getByRole('button', { name: 'Save changes' }))
  await waitFor(() => expect(requests.some((request) => request.operation === 'agent.save')).toBe(true))
  const body = requests.find((request) => request.operation === 'agent.save').body.agent
  expect(body.name).toBe('Inbox digest')
  expect(body.routines.map((routine) => routine.id)).toEqual(['extract-1', 'extract-9f2c', added.id])
  expect(body.routines[1]).toMatchObject({ enabled: false, proposed: true })
  expect(body.routines[2]).toMatchObject({ enabled: true, trigger: added.trigger })
})

it('drops a routine an earlier draft named when the newest draft changes it, and keeps the rest', () => {
  const proposal = { id: 'extract-9f2c', name: 'List what needs a reply', enabled: false, proposed: true, trigger: { kind: 'cron', expr: '0 17 * * 5', tz: 'America/New_York' }, prompt: 'List what needs a reply.' }
  const added = { id: 'r-user', name: 'Noon check', enabled: true, trigger: { kind: 'cron', expr: '0 12 * * *', tz: 'America/New_York' }, prompt: 'Check the inbox.' }
  const saved = { ...draftAgent(draftForm(draft)), id: 'agent-7', routines: [{ ...draft.routines[0] }, proposal, added] }
  const moved = { ...draft.routines[0], id: 'extract-2', trigger: { ...draft.routines[0].trigger, expr: '0 8 * * 1-5' } }
  const body = draftAgent(draftForm({ ...draft, routines: [moved] }), { saved, earlier: draft.routines })
  expect(body.routines.map((routine) => routine.id)).toEqual(['extract-2', 'extract-9f2c', 'r-user'])
  expect(draftAgent(draftForm({ ...draft, routines: [] }), { saved, earlier: draft.routines }).routines.map((routine) => routine.id)).toEqual(['extract-9f2c', 'r-user'])
  expect(sameAgent(draftAgent(draftForm(draft), { saved, earlier: draft.routines }), saved)).toBe(true)
})

it('rests Save on the draft it saved until a new message drafts a change', async () => {
  const { tauri } = runtime()
  const first = [turn('Summarize my inbox')]
  const { rerender } = render(AgentDraftChat, { tauri, turns: first })
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
  expect(await screen.findByRole('status')).toHaveTextContent('Saved.')
  const rest = screen.getByRole('button', { name: 'Save changes' })
  expect(rest).toBeDisabled()
  expect(rest).toHaveAttribute('data-variant', 'outline')
  expect(screen.getByRole('button', { name: 'Test run' })).toBeEnabled()
  await rerender({ turns: [...first, { ...turn('Call it Inbox digest', { draft: { ...draft, name: 'Inbox digest' }, question: null, via: 'router' }), at: '2026-10-19T10:05:00Z' }] })
  expect(screen.getByRole('button', { name: 'Save changes' })).toBeEnabled()
  // An unsaved change clears Saved. under the waking Save changes.
  expect(screen.queryByText('Saved.')).toBeNull()
})

it('clears a Save error when the next draft arrives', async () => {
  const { tauri } = runtime()
  const invoke = tauri.invoke.getMockImplementation()
  tauri.invoke.mockImplementation(async (command, args) => {
    if (args.operation === 'agent.save') throw new Error('Remove credentials from the agent instructions.')
    return invoke(command, args)
  })
  const first = [turn('Summarize my inbox')]
  const { rerender } = render(AgentDraftChat, { tauri, turns: first })
  await fireEvent.click(screen.getByRole('button', { name: 'Save' }))
  expect(await screen.findByRole('alert')).toHaveTextContent('Remove credentials from the agent instructions.')
  await rerender({ turns: [...first, { ...turn('Call it Inbox digest', { draft: { ...draft, name: 'Inbox digest' }, question: null, via: 'router' }), at: '2026-10-19T10:05:00Z' }] })
  expect(screen.queryByRole('alert')).toBeNull()
  expect(screen.getByRole('button', { name: 'Save' })).toBeEnabled()
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

it('reads an agent save body as unchanged only when every field a person sets matches', () => {
  const saved = { ...draftAgent(draftForm(draft)), id: 'agent-7', permissions: { mode: 'manual' }, routines: draft.routines.map((routine) => ({ ...routine, proposed: false, next_fire_at: '2026-10-20T13:00:00Z' })) }
  const body = (change = {}) => draftAgent(draftForm({ ...draft, ...change }), { saved })
  expect(sameAgent(body(), saved)).toBe(true)
  expect(sameAgent(body({ label: 'Evening briefer' }), saved)).toBe(false)
  expect(sameAgent(body({ instructions: 'List the mail.' }), saved)).toBe(false)
  expect(sameAgent(body({ skills: [] }), saved)).toBe(false)
  expect(sameAgent(body({ permission_mode: 'auto' }), saved)).toBe(false)
  // A draft that drops the routines an earlier draft named changes them.
  expect(sameAgent(draftAgent(draftForm({ ...draft, routines: [] }), { saved, earlier: draft.routines }), saved)).toBe(false)
  expect(sameAgent(draftAgent(draftForm(draft), { saved, test: true }), saved)).toBe(false)
  expect(sameAgent(body(), null)).toBe(false)
})

it('keeps the earlier draft after a failed draft and sends the message again from Try again', async () => {
  const { tauri } = runtime()
  const onsend = vi.fn()
  render(AgentDraftChat, { tauri, turns: [turn('Summarize my inbox')], error: 'The draft could not be made. Try again.', failed: 'Also read my calendar', onsend })
  expect(screen.getByRole('alert')).toHaveTextContent('The draft could not be made. Try again.')
  const drafts = screen.getAllByRole('article')
  expect(drafts).toHaveLength(1)
  expect(drafts[0]).toHaveAccessibleName('Draft: Inbox brief')
  expect(within(drafts[0]).getByText(draft.instructions)).toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', { name: 'Try again' }))
  expect(onsend).toHaveBeenCalledWith('Also read my calendar')
})
