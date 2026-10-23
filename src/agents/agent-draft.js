// Agent creation in a chat. Core's `agent.draft` turns the messages of an
// agent's creation chat into a filled agent form with one model call on its
// own thread: the first request answers a draft id at once, and each later
// request with that id answers until the draft is done. Core keeps each
// message and its draft in the chat's creation record. This module asks,
// shapes the form and builds the `agent.save` body. Core owns the draft, the
// trigger reading and every check on save.
import { agentRequest } from './agent-runtime.js'

// How often the builder asks after a draft, and how long it waits in all.
export const DRAFT_POLL_MS = 250
export const DRAFT_TIMEOUT_MS = 90_000

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

// The draft for `source`: `{ chatId, message }` for the next message of a
// creation chat, with `threadId` on a first message that starts from a
// thread, or `{ description }` or `{ threadId }` alone. Answers `{ draft,
// question, via, creation }`, where `creation` is a chat's record.
export async function requestDraft(tauri, runtime, source, { answers = [], wait = pause, now = Date.now, cancelled = () => false } = {}) {
  const body = source?.chatId
    ? { chat_id: source.chatId, message: String(source.message ?? ''), ...(source.threadId ? { thread_id: source.threadId } : {}) }
    : source?.threadId ? { thread_id: source.threadId } : { description: String(source?.description ?? '') }
  if (answers.length) body.answers = answers
  let answer = await agentRequest(tauri, runtime, 'agent.draft', body)
  const started = now()
  while (answer?.status === 'drafting') {
    if (cancelled()) return null
    if (now() - started > DRAFT_TIMEOUT_MS) throw new Error('The draft took too long. Send the message again.')
    await wait(DRAFT_POLL_MS)
    if (cancelled()) return null
    answer = await agentRequest(tauri, runtime, 'agent.draft', { draft_id: answer.draft_id })
  }
  if (!answer?.draft) throw new Error('The machine answered no draft. Try again.')
  return { draft: answer.draft, question: answer.question ?? null, via: answer.via ?? '', creation: answer.creation ?? null }
}

// The editable form for a draft. Each routine keeps its trigger and starts on.
export function draftForm(draft) {
  return {
    name: draft?.name ?? '',
    label: draft?.label ?? '',
    instructions: draft?.instructions ?? '',
    skills: [...(draft?.skills ?? [])],
    mode: draft?.permission_mode ?? '',
    routines: (draft?.routines ?? []).map((routine) => ({ ...routine, enabled: routine.enabled !== false })),
  }
}

// What the form still needs before a save: core needs a name and instructions.
export function formProblem(form) {
  if (!form?.name?.trim()) return 'Give the agent a name.'
  if (!form?.instructions?.trim()) return 'Give the agent instructions.'
  return ''
}

// The `agent.save` body for the form. `saved` is the agent an earlier save
// answered, so a second save keeps its id and avatar. A test save keeps every
// routine off, so the agent starts itself only after Save.
// A new agent with `projectId` works in that project, so the project gains its triggers.
export function draftAgent(form, { saved = null, test = false, projectId = null } = {}) {
  const agent = {
    ...(saved ?? { id: '', projectId }),
    name: form.name.trim(),
    label: form.label.trim(),
    instructions: form.instructions.trim(),
    schedule: null,
    skills: [...form.skills],
    routines: form.routines.map((routine) => ({ ...routine, enabled: test ? false : routine.enabled, proposed: false })),
  }
  if (form.mode) agent.permissions = { mode: form.mode }
  return agent
}

// The line a test run shows while it runs and after: `{ text, done, failed }`.
// Its triggers stay off until Save, so the line says so until the run ends.
export function testRunLine(run) {
  const status = run?.status ?? 'queued'
  if (status === 'completed') return { text: run.silent ? 'Test run finished with nothing to post.' : 'Test run finished.', done: true, failed: false }
  if (status === 'failed' || status === 'interrupted') return { text: run.error ? `Test run failed. ${run.error}` : 'Test run failed.', done: true, failed: true }
  if (status === 'cancelled') return { text: 'Test run cancelled.', done: true, failed: false }
  if (status === 'waiting_input') return { text: 'Test run waits for your answer.', done: false, failed: false }
  if (status === 'running') return { text: 'Test run running. Its triggers stay off until you save.', done: false, failed: false }
  return { text: 'Test run started. Its triggers stay off until you save.', done: false, failed: false }
}
