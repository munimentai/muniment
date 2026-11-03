<script>
  // One agent approval: the agent with its activity, its machine and age,
  // the tool call as core presents it (its headline, why the gate asks and
  // its details, with the exact request behind Show details), a plan waiting
  // for approval, and only the answers core accepts. Core applies the first
  // answer from any device. A later one comes back as already answered and
  // its line shows here.
  import Button from '../lib/ui/Button.svelte'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import MachineMark from './MachineMark.svelte'
  import { agentRequest } from './agent-runtime.js'
  import { approvalChoices, approvalForm, approvalOutcome, approvalSummary, longInput } from './approvals.js'
  import { parseQuestions, pickOption, questionAnswerValue, questionsComplete } from '../lib/user-question.js'
  import { whenLabel } from '../lib/agent-labels.js'
  import ToolCall from '../activity/ToolCall.svelte'
  import { ruleInput } from '../activity/tool-call.js'

  let { tauri, approval, agent = null, activity = undefined, machine = null, inline = false, onanswered = () => {}, onopen = null } = $props()
  let busy = $state(false)
  let note = $state('')
  let settled = $state(false)
  let open = $state(false)
  const summary = $derived(approvalSummary(approval))
  const choices = $derived(approvalChoices(approval))
  const folds = $derived(summary.rows.some(([, value]) => longInput(value)))
  const plan = $derived(String(approval.plan ?? '').trim())
  let planOpen = $state(false)
  const shown = $derived(agent ?? { id: approval.agent_id, name: approval.agent_name })
  // A question or text request takes its answer here, as its conversation
  // does, so a run with no conversation can still go on.
  const form = $derived(approvalForm(approval))
  const questions = $derived(form === 'question' ? parseQuestions(approval.request?.prefill) : [])
  let answers = $state({})
  let text = $state('')
  const complete = $derived(questionsComplete(questions, answers))
  const answerText = (question, value) => { answers = { ...answers, [question.id]: { selected: [], text: '', ...answers[question.id], text: value } } }
  const conversation = $derived(!!approval.thread_id)

  async function answer(choice) {
    busy = true
    note = ''
    try {
      const result = await agentRequest(tauri, approval.runtimeId, 'agent.approval_answer', { approval_id: approval.approval_id, answer: choice.answer })
      note = approvalOutcome(result?.status)
      settled = result?.status === 'already_answered'
      onanswered(approval, result, note ? `${approval.agent_name}: ${note}` : '')
    } catch (failure) {
      note = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }
</script>

<article class="approval" class:inline aria-label={`Approval for ${approval.agent_name}`}>
  <header class="who">
    <AgentAvatar agent={shown} {activity} size={20} /><strong>{approval.agent_name}</strong>
    {#if machine}<MachineMark {machine} />{/if}
    <time class="when" datetime={approval.created_at}>{whenLabel(approval.created_at)}</time>
  </header>
  {#if form === 'question'}
    <p class="operation human">{approval.agent_name} asks you</p>
    {#if questions.length}
      {#each questions as question (question.id)}
        <fieldset class="question" disabled={busy || settled}>
          <legend>{question.question}</legend>
          {#if question.options?.length}
            <div class="options" role="group" aria-label={question.question}>
              {#each question.options as option (option.label)}
                <Button variant="outline" aria-pressed={answers[question.id]?.selected?.includes(option.label) ?? false} title={option.description ?? undefined} onclick={() => { answers = pickOption(answers, question, option.label) }}>{option.label}</Button>
              {/each}
            </div>
          {/if}
          <textarea class="field" rows="2" aria-label={`Your answer: ${question.question}`} placeholder="Write an answer or add details" value={answers[question.id]?.text ?? ''} oninput={(event) => answerText(question, event.currentTarget.value)}></textarea>
        </fieldset>
      {/each}
    {:else}<p class="note" role="alert">The questions could not be displayed.</p>{/if}
  {:else if form === 'text'}
    <p class="operation">{summary.title}</p>
    <textarea class="field" rows="3" aria-label={summary.title} placeholder={approval.request?.placeholder ?? ''} disabled={busy || settled} bind:value={text}></textarea>
  {:else if form === 'browser'}
    <p class="operation human">{approval.agent_name} is using the browser.</p>
  {:else if approval.presentation && !plan}
    <ToolCall variant="card" presentation={approval.presentation} reason={approval.reason ?? ''} input={ruleInput(approval.rule)} />
  {:else}
  <p class="operation">{plan ? 'Approve this plan?' : summary.operation}</p>
  {#if approval.reason}<p class="reason">{approval.reason}</p>{/if}
  {/if}
  {#if plan}
    <div class="plan" class:folded={longInput(plan, 14, 900) && !planOpen} aria-label="Plan" role="region">{plan}</div>
    {#if longInput(plan, 14, 900)}<button type="button" class="more" aria-expanded={planOpen} onclick={() => { planOpen = !planOpen }}>{planOpen ? 'Show less' : 'Show all'}</button>{/if}
  {/if}
  {#if summary.rows.length && !plan && !approval.presentation}
    <dl class="inputs" class:folded={folds && !open}>
      {#each summary.rows as [label, value] (label)}<div><dt>{label}</dt><dd>{value}</dd></div>{/each}
    </dl>
    {#if folds}<button type="button" class="more" aria-expanded={open} onclick={() => { open = !open }}>{open ? 'Show less' : 'Show all'}</button>{/if}
  {/if}
  {#if !settled}
    <div class="actions">
      {#each choices as choice (choice.label)}
        <Button variant={choice.tone === 'deny' ? 'outline' : 'default'} disabled={busy} onclick={() => answer(choice)}>{choice.label}</Button>
      {/each}
      {#if form === 'question' && questions.length}
        <Button disabled={busy || !complete} onclick={() => answer({ answer: { type: 'editor', value: questionAnswerValue(questions, answers) } })}>{busy ? 'Sending…' : 'Send answers'}</Button>
      {:else if form === 'text'}
        <Button disabled={busy} onclick={() => answer({ answer: { type: approval.request.kind, value: text } })}>Submit</Button>
      {/if}
      {#if approval.request?.kind === 'code_diff'}
        <p class="note">{conversation ? 'Review the file changes in its conversation.' : 'This run has no conversation to show its file changes in. Deny ends the wait.'}</p>
      {:else if !choices.length && !form}
        <p class="note">{conversation ? 'Answer this request in its conversation.' : 'This run has no conversation, and this request takes no answer here. The run ends the wait when its unattended time runs out.'}</p>
      {/if}
      {#if onopen}<Button variant="outline" disabled={busy} onclick={() => onopen(approval)}>Open conversation</Button>{/if}
    </div>
  {/if}
  {#if note}<p class="note" role="status">{note}</p>{/if}
</article>

<style>
  .approval { display: grid; gap: 8px; padding: 12px 0; min-width: 0; }
  .approval.inline { padding: 12px 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  .who { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; font-size: var(--text-13); }
  .when { color: var(--muted); font: var(--text-12) var(--font-human); }
  .operation { margin: 0; font: var(--text-13) var(--font-mono); overflow-wrap: anywhere; }
  .reason { margin: 0; color: var(--muted); font-size: var(--text-12); line-height: 1.4; overflow-wrap: anywhere; }
  .plan { padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); font-size: var(--text-13); line-height: 1.55; white-space: pre-wrap; overflow-wrap: anywhere; }
  .plan.folded { max-height: 15.5em; overflow: hidden; mask-image: linear-gradient(to bottom, currentColor 70%, transparent); }
  .inputs { display: grid; gap: 4px; margin: 0; padding: 8px 10px; border-radius: var(--radius-control); background: var(--faint); font: var(--text-12) var(--font-mono); }
  .inputs.folded { max-height: 7.5em; overflow: hidden; mask-image: linear-gradient(to bottom, currentColor 70%, transparent); }
  .inputs div { display: grid; grid-template-columns: 72px minmax(0, 1fr); gap: 8px; }
  dt { color: var(--muted); }
  dd { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  .more { justify-self: start; min-height: 24px; padding: 2px 6px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: inherit; font-size: var(--text-12); cursor: pointer; }
  .more:hover { background: var(--faint); color: var(--ink); }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .note { margin: 0; color: var(--muted); font-size: var(--text-12); }
  .operation.human { font-family: var(--font-human); }
  .question { display: grid; gap: 6px; min-width: 0; margin: 0; padding: 0; border: 0; }
  legend { padding: 0; margin-bottom: 6px; font-size: var(--text-13); overflow-wrap: anywhere; }
  .options { display: flex; flex-wrap: wrap; gap: 6px; }
  .field { box-sizing: border-box; width: 100%; min-width: 0; resize: vertical; padding: 6px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); color: var(--ink); font: var(--text-13) var(--font-human); }
</style>
