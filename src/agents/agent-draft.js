// One-sentence agent creation. Core's `agent.draft` turns a description, or a
// thread's text, into a filled agent form with one model call on its own
// thread: the first request answers a draft id at once, and each later
// request with that id answers until the draft is done. This module asks,
// shapes the form and builds the `agent.save` body. Core owns the draft, the
// trigger reading and every check on save.
import { agentRequest } from './agent-runtime.js'

// How often the builder asks after a draft, and how long it waits in all.
export const DRAFT_POLL_MS = 250
export const DRAFT_TIMEOUT_MS = 90_000

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

// The draft for `source`, `{ description }` or `{ threadId }`, with any
// answers to its earlier question. Answers `{ draft, question, via }`.
export async function requestDraft(tauri, runtime, source, { answers = [], wait = pause, now = Date.now, cancelled = () => false } = {}) {
  const body = source?.threadId ? { thread_id: source.threadId } : { description: String(source?.description ?? '') }
  if (answers.length) body.answers = answers
  let answer = await agentRequest(tauri, runtime, 'agent.draft', body)
  const started = now()
  while (answer?.status === 'drafting') {
    if (cancelled()) return null
    if (now() - started > DRAFT_TIMEOUT_MS) throw new Error('The draft took too long. Try again, or refine it in chat.')
    await wait(DRAFT_POLL_MS)
    if (cancelled()) return null
    answer = await agentRequest(tauri, runtime, 'agent.draft', { draft_id: answer.draft_id })
  }
  if (!answer?.draft) throw new Error('The machine answered no draft. Try again.')
  return { draft: answer.draft, question: answer.question ?? null, via: answer.via ?? '' }
}

// The editable form for a draft. Each routine keeps its trigger and starts on.
export function draftForm(draft) {
  return {
    name: draft?.name ?? '',
    label: draft?.label ?? '',
    instructions: draft?.instructions ?? '',
    skills: [...(draft?.skills ?? [])],
    toolsOff: [...(draft?.tools_off ?? [])],
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
export function draftAgent(form, { saved = null, test = false } = {}) {
  const agent = {
    ...(saved ?? { id: '', project_id: null }),
    name: form.name.trim(),
    label: form.label.trim(),
    instructions: form.instructions.trim(),
    schedule: null,
    skills: [...form.skills],
    tools: { disabled: [...form.toolsOff] },
    routines: form.routines.map((routine) => ({ ...routine, enabled: test ? false : routine.enabled, proposed: false })),
  }
  if (form.mode) agent.permissions = { always_allow: [], ask_first: [], ...(saved?.permissions ?? {}), mode: form.mode }
  return agent
}

// The answer an option or typed text gives to a draft's question.
export function questionAnswer(question, answer) {
  return { question: question.text, answer: String(answer ?? '').trim() }
}
