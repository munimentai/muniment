<script>
  // One-sentence agent creation. Describe your agent takes a sentence, or a
  // thread stands in for it, and core's `agent.draft` fills the agent form
  // with one model call. The form shows inline for review: name, job title,
  // instructions, triggers, tools, skills and permission mode. A question
  // shows only when core could not tell the job. Run test now saves the agent
  // with its triggers off and starts one test run, whose line follows the
  // run from `runs`, the window's agent runs, to its result with a link to
  // its thread; Save turns the triggers on.
  // Refine in chat hands the words to the creation chat instead. New agent,
  // the thread starters, the templates and a thread's Make this an agent all
  // open this one component.
  import { onMount, tick } from 'svelte'
  import '../lib/ui/settings-controls.css'
  import PanelPage from '../lib/ui/PanelPage.svelte'
  import PageHead from '../lib/ui/PageHead.svelte'
  import Button from '../lib/ui/Button.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import { agentRequest } from './agent-runtime.js'
  import { draftAgent, draftForm, formProblem, questionAnswer, requestDraft, testRunLine } from './agent-draft.js'
  import FailureNote from '../lib/FailureNote.svelte'
  import { emptyCatalog, loadToolCatalog } from './agent-tools.js'
  import { loadModes } from './permission-modes.js'
  import { triggerSummary } from './routine-labels.js'
  import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'

  let {
    tauri,
    runtime = LOCAL_RUNTIME_ID,
    description: initial = '',
    threadId = null,
    threadTitle = '',
    autostart = false,
    projectId = null,
    runs = [],
    onsaved = () => {},
    onrefine = null,
    onopenchat = () => {},
    onchange = () => {},
  } = $props()

  // svelte-ignore state_referenced_locally
  let description = $state(initial)
  let phase = $state('describe')
  let form = $state(null)
  let question = $state(null)
  let answers = $state([])
  let typedAnswer = $state('')
  let via = $state('')
  let error = $state('')
  let busy = $state(false)
  let saved = $state(null)
  let testRun = $state(null)
  let catalog = $state(emptyCatalog())
  let modes = $state(null)
  let input = $state()
  let alive = true
  const id = $props.id()
  const drafting = $derived(phase === 'drafting')
  const fromThread = $derived(!!threadId)

  onMount(() => {
    void loadToolCatalog(tauri, runtime).then((value) => { if (alive) catalog = value })
    void loadModes(tauri, runtime).then((value) => { if (alive) modes = value })
    if (autostart && (fromThread || description.trim())) void draft()
    else void tick().then(() => input?.focus())
    return () => { alive = false }
  })

  async function draft(given = []) {
    if (!fromThread && !description.trim()) { error = 'Describe what the agent does.'; return }
    phase = 'drafting'
    error = ''
    try {
      const answer = await requestDraft(tauri, runtime, fromThread ? { threadId } : { description }, { answers: given, cancelled: () => !alive })
      if (!answer || !alive) return
      form = draftForm(answer.draft)
      question = answer.question
      answers = given
      via = answer.via
      typedAnswer = ''
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      if (alive) phase = form ? 'review' : 'describe'
    }
  }

  function answer(text) {
    if (!question || !String(text ?? '').trim()) return
    void draft([...answers, questionAnswer(question, text)])
  }

  function describeKeydown(event) {
    if (event.key !== 'Enter' || event.shiftKey || event.isComposing) return
    event.preventDefault()
    if (!drafting) void draft()
  }

  function toggleTool(tool, on) {
    form.toolsOff = on ? form.toolsOff.filter((entry) => entry !== tool) : [...form.toolsOff, tool]
  }

  function toggleSkill(skill, on) {
    form.skills = on ? [...form.skills, skill] : form.skills.filter((entry) => entry !== skill)
  }

  // A test save keeps the triggers off, then starts one run of the agent.
  async function save(test) {
    const problem = formProblem(form)
    if (problem) { error = problem; return }
    busy = true
    error = ''
    try {
      const answer = await agentRequest(tauri, runtime, 'agent.save', { agent: draftAgent($state.snapshot(form), { saved: $state.snapshot(saved), test, projectId }) })
      saved = answer.agent
      onchange({ ...saved, runtimeId: runtime })
      if (test) {
        const started = await agentRequest(tauri, runtime, 'agent.run_now', { id: saved.id, test: true })
        testRun = started.run ?? null
      } else {
        onsaved({ ...saved, runtimeId: runtime })
      }
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }

  function refine() {
    onrefine?.(fromThread ? (form?.instructions ?? '') : description)
  }

  const modeOptions = $derived((modes?.modes ?? []).filter((mode) => mode.available !== false))
  const defaultMode = $derived(modes?.modes?.find((mode) => mode.id === modes?.default_agent_mode)?.title ?? '')
  const toolsShown = $derived(catalog.builtin.filter((tool) => tool.id !== 'ask_user_question'))
  // The test run as the window last heard of it.
  const testLatest = $derived(testRun ? (runs.find((run) => run.id === testRun.id && (run.runtimeId ?? LOCAL_RUNTIME_ID) === runtime) ?? testRun) : null)
  const testLine = $derived(testLatest ? testRunLine(testLatest) : null)
</script>

<section data-panel="agent-builder" class="agent-builder" aria-label="New agent">
  <PanelPage data-settings-controls>
    <PageHead title="New agent" level={2} intro="Describe the job in one sentence. The form fills in for you to review before you save." />
    <div class="column">
      {#if fromThread}
        <p class="source">From <strong>{threadTitle || 'this thread'}</strong></p>
      {:else}
        <form class="describe" onsubmit={(event) => { event.preventDefault(); if (!drafting) void draft() }}>
          <label for={`${id}-describe`}>Describe your agent</label>
          <textarea id={`${id}-describe`} bind:this={input} bind:value={description} rows="2" maxlength="4000" disabled={drafting || busy}
            placeholder="Every weekday at 9am, summarize my inbox and list what needs a reply" onkeydown={describeKeydown}></textarea>
          <div class="actions">
            <Button type="submit" variant={form ? 'outline' : 'default'} disabled={drafting || busy || !description.trim()}>{form ? 'Draft again' : 'Draft'}</Button>
            {#if onrefine && !form}<Button variant="outline" disabled={drafting || busy} onclick={refine}>Refine in chat</Button>{/if}
          </div>
        </form>
      {/if}

      {#if drafting}<p class="state" role="status">Drafting the agent…</p>{/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}

      {#if question && !drafting}
        <fieldset class="question" aria-label="Question">
          <legend>{question.text}</legend>
          <div class="options">{#each question.options as option (option)}<Button variant="outline" disabled={busy} onclick={() => answer(option)}>{option}</Button>{/each}</div>
          <form class="typed" onsubmit={(event) => { event.preventDefault(); answer(typedAnswer) }}>
            <input aria-label="Your answer" bind:value={typedAnswer} maxlength="400" placeholder="Or type an answer" disabled={busy} />
            <Button type="submit" variant="outline" disabled={busy || !typedAnswer.trim()}>Answer</Button>
          </form>
        </fieldset>
      {/if}

      {#if form && !drafting}
        <form aria-label="Agent draft" onsubmit={(event) => { event.preventDefault(); void save(false) }}>
          <div class="pair">
            <label>Name<input bind:value={form.name} maxlength="100" disabled={busy} /></label>
            <label>Job title<input bind:value={form.label} maxlength="120" disabled={busy} /></label>
          </div>
          <label>Instructions<textarea bind:value={form.instructions} rows="7" disabled={busy}></textarea></label>

          <fieldset>
            <legend>Triggers</legend>
            {#each form.routines as routine, index (routine.id || index)}
              <div class="row"><Toggle bind:checked={form.routines[index].enabled} label={`Run ${routine.name}`} disabled={busy} /><span class="what">{routine.name}</span><span class="record">{triggerSummary(routine.trigger)}</span></div>
            {:else}
              <p class="state">Runs when you ask.</p>
            {/each}
          </fieldset>

          {#if toolsShown.length}
            <fieldset>
              <legend>Tools</legend>
              <div class="chips">{#each toolsShown as tool (tool.id)}<Button variant="capability" aria-pressed={!form.toolsOff.includes(tool.id)} disabled={busy} onclick={() => toggleTool(tool.id, form.toolsOff.includes(tool.id))}>{tool.label}</Button>{/each}</div>
            </fieldset>
          {/if}

          {#if catalog.skills.length}
            <fieldset>
              <legend>Skills</legend>
              <div class="chips">{#each catalog.skills as skill (skill.id)}<Button variant="capability" aria-pressed={form.skills.includes(skill.id)} disabled={busy} title={skill.description} onclick={() => toggleSkill(skill.id, !form.skills.includes(skill.id))}>{skill.name}</Button>{/each}</div>
            </fieldset>
          {/if}

          {#if modeOptions.length}
            <label>Permission mode
              <select bind:value={form.mode} disabled={busy}>
                <option value="">{defaultMode ? `Default: ${defaultMode}` : 'Default'}</option>
                {#each modeOptions as mode (mode.id)}<option value={mode.id}>{mode.title}</option>{/each}
              </select>
            </label>
          {/if}

          {#if via === 'rules'}<p class="state">No model answered, so the form holds your words.</p>{/if}

          <div class="actions">
            <Button type="submit" disabled={busy}>Save</Button>
            <Button variant="outline" icon="play" disabled={busy} onclick={() => save(true)}>Run test now</Button>
            {#if onrefine}<Button disabled={busy} onclick={refine}>Refine in chat</Button>{/if}
          </div>
          {#if testLine}
            <div class="state" role="status">
              {#if testLine.failed}<FailureNote failure={testLine.text} role="none" />{:else}<span>{testLine.text}</span>{/if}
              <Button disabled={busy} onclick={() => onopenchat({ ...saved, runtimeId: runtime }, testLatest)}>{testLine.done ? 'Open result' : 'Open chat'}</Button>
            </div>
          {/if}
        </form>
      {/if}
    </div>
  </PanelPage>
</section>

<style>
  .agent-builder { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  .column { display: grid; gap: 16px; width: 100%; max-width: 760px; }
  form, fieldset { display: grid; gap: 10px; margin: 0; padding: 0; border: 0; min-width: 0; }
  label { display: grid; gap: 4px; }
  .describe > label { color: var(--ink); font: var(--weight-semibold) var(--text-15) var(--font-human); }
  textarea, input, select { width: 100%; }
  textarea { font-family: var(--font-human); line-height: var(--leading-body); resize: vertical; }
  .describe textarea { font-size: var(--text-15); }
  legend { margin-bottom: 6px; padding: 0; color: var(--muted); font: var(--text-13) var(--font-human); }
  .pair { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 10px; }
  .row { display: flex; align-items: center; gap: 10px; min-height: 30px; font-size: var(--text-13); }
  .what { flex: 1; min-width: 0; color: var(--ink); overflow-wrap: break-word; }
  .record { flex: none; margin-left: auto; color: var(--muted); font: var(--text-12) var(--font-mono); white-space: nowrap; }
  .chips, .options { display: flex; flex-wrap: wrap; gap: 6px; }
  .typed { display: flex; gap: 8px; }
  .typed input { flex: 1; }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .source { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .source strong { color: var(--ink); }
  .state { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin: 0; color: var(--muted); font-size: var(--text-13); }
  .error { margin: 0; color: var(--oxide); font-size: var(--text-13); }
  .question { padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  .question legend { float: left; width: 100%; margin-bottom: 8px; color: var(--ink); font-weight: var(--weight-semibold); }
  @media (max-width: 560px) { .pair { grid-template-columns: minmax(0, 1fr); } }
</style>
