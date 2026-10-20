<script>
  // A new agent's chat, drawn in the transcript like any chat. Each message
  // the user sends runs core's `agent.draft`, and its reply is the agent's
  // draft: its name, what it does, when it runs, its skills and its model.
  // The newest reply holds Save and Test run. Test run saves the agent with
  // its triggers off and starts one run, whose line follows the run from
  // `runs`, the window's agent runs. Save turns the triggers on. The chat's
  // creation record keeps every message and draft, so the chat stays to
  // revise the agent, and each later Save updates the same agent.
  import { onMount } from 'svelte'
  import Button from '../lib/ui/Button.svelte'
  import FailureNote from '../lib/FailureNote.svelte'
  import { agentRequest } from './agent-runtime.js'
  import { draftAgent, draftForm, formProblem, testRunLine } from './agent-draft.js'
  import { emptyCatalog, loadToolCatalog } from './agent-tools.js'
  import { triggerSummary } from './routine-labels.js'
  import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'

  // `turns` are the creation record's `{ message, at, reply }`. `pending` is
  // the message a draft is answering. `agentId` is the agent a Save made.
  // `model` names the model the agent runs.
  let {
    tauri, runtime = LOCAL_RUNTIME_ID, turns = [], pending = '', error = '', agentId = null, projectId = null, model = '', runs = [],
    onsend = () => {}, onsaved = () => {}, onopenchat = () => {},
  } = $props()

  let catalog = $state(emptyCatalog())
  let busy = $state(false)
  let saveError = $state('')
  let saved = $state(null)
  let testRun = $state(null)
  let alive = true

  onMount(() => {
    void loadToolCatalog(tauri, runtime).then((value) => { if (alive) catalog = value })
    return () => { alive = false }
  })

  const latest = $derived(turns.length - 1)
  const skillNames = (draft) => (draft?.skills ?? []).map((id) => catalog.skills.find((skill) => skill.id === id)?.name ?? id)
  const schedule = (draft) => (draft?.routines ?? []).map((routine) => triggerSummary(routine.trigger)).join(', ') || 'Runs when you ask.'
  // The test run as the window last heard of it.
  const testLatest = $derived(testRun ? (runs.find((run) => run.id === testRun.id && (run.runtimeId ?? LOCAL_RUNTIME_ID) === runtime) ?? testRun) : null)
  const testLine = $derived(testLatest ? testRunLine(testLatest) : null)

  // A test save keeps the triggers off, then starts one run of the agent.
  async function save(test) {
    const form = draftForm($state.snapshot(turns[latest]?.reply?.draft))
    const problem = formProblem(form)
    if (problem) { saveError = problem; return }
    busy = true
    saveError = ''
    try {
      const id = saved?.id ?? agentId
      const previous = id ? await agentRequest(tauri, runtime, 'agent.read', { id }).then((answer) => answer?.agent ?? null).catch(() => null) : null
      const answer = await agentRequest(tauri, runtime, 'agent.save', { agent: draftAgent(form, { saved: $state.snapshot(previous), test, projectId }) })
      saved = answer.agent
      if (test) {
        const started = await agentRequest(tauri, runtime, 'agent.run_now', { id: saved.id, test: true })
        testRun = started.run ?? null
      }
      await onsaved({ ...saved, runtimeId: runtime }, { test })
    } catch (failure) {
      saveError = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }
</script>

<div class="agent-chat" aria-label="New agent chat">
  {#each turns as turn, index (turn.at + index)}
    {@const reply = turn.reply ?? {}}
    {@const draft = reply.draft}
    <div class="user-turn"><div class="user-message"><p>{turn.message}</p></div></div>
    {#if draft}
      <article class="draft" aria-label={`Draft: ${draft.name}`}>
        <h3>{draft.name}</h3>
        {#if draft.label}<p class="label">{draft.label}</p>{/if}
        <dl>
          <dt>What it does</dt><dd class="instructions">{draft.instructions}</dd>
          <dt>When</dt><dd>{schedule(draft)}</dd>
          {#if skillNames(draft).length}<dt>Skills</dt><dd>{skillNames(draft).join(', ')}</dd>{/if}
          {#if model}<dt>Model</dt><dd>{model}</dd>{/if}
        </dl>
        {#if reply.via === 'rules'}<p class="state">No model answered, so the draft holds your words.</p>{/if}
        {#if reply.question}
          <div class="question" role="group" aria-label="Question">
            <p>{reply.question.text}</p>
            {#if index === latest}<div class="actions">{#each reply.question.options as option (option)}<Button variant="outline" disabled={busy || !!pending} onclick={() => onsend(option)}>{option}</Button>{/each}</div>{/if}
          </div>
        {/if}
        {#if index === latest}
          <div class="actions">
            <Button variant="primary" disabled={busy || !!pending} onclick={() => save(false)}>{saved || agentId ? 'Save changes' : 'Save'}</Button>
            <Button variant="outline" icon="play" disabled={busy || !!pending} onclick={() => save(true)}>Test run</Button>
          </div>
          {#if saveError}<p class="error" role="alert">{saveError}</p>{/if}
          {#if testLine}
            <div class="state" role="status">
              {#if testLine.failed}<FailureNote failure={testLine.text} role="none" />{:else}<span>{testLine.text}</span>{/if}
              <Button disabled={busy} onclick={() => onopenchat({ ...saved, runtimeId: runtime }, testLatest)}>{testLine.done ? 'Open result' : 'Open chat'}</Button>
            </div>
          {:else if saved}
            <div class="state" role="status"><span>Saved.</span><Button disabled={busy} onclick={() => onopenchat({ ...saved, runtimeId: runtime }, null)}>Open agent</Button></div>
          {/if}
        {/if}
      </article>
    {:else if reply.error}
      <p class="error">{reply.error}</p>
    {/if}
  {/each}
  {#if pending}
    <div class="user-turn"><div class="user-message"><p>{pending}</p></div></div>
    <p class="state" role="status">Drafting the agent…</p>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>

<style>
  .agent-chat { display: grid; gap: 16px; }
  .user-turn { display: flex; justify-content: flex-end; }
  .user-message { width: fit-content; max-width: 78%; padding: 9px 13px; overflow-wrap: anywhere; background: var(--faint); border-radius: var(--radius-panel); }
  .user-message p { margin: 0; white-space: pre-wrap; }
  .draft { display: grid; gap: 8px; min-width: 0; }
  h3 { margin: 0; font: var(--weight-semibold) var(--text-17) var(--font-heading); line-height: var(--leading-heading); }
  .label { margin: -4px 0 0; color: var(--muted); font-size: var(--text-13); }
  dl { display: grid; grid-template-columns: max-content minmax(0, 1fr); gap: 6px 16px; margin: 0; }
  dt { color: var(--muted); font-size: var(--text-13); }
  dd { margin: 0; overflow-wrap: anywhere; }
  .instructions { white-space: pre-wrap; }
  .question { display: grid; gap: 8px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  .question p { margin: 0; font-weight: var(--weight-semibold); }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .state { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin: 0; color: var(--muted); font-size: var(--text-13); }
  .error { margin: 0; color: var(--oxide); font-size: var(--text-13); }
  @media (max-width: 560px) { dl { grid-template-columns: minmax(0, 1fr); } }
</style>
