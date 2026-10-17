<script>
  // One agent approval: the agent with its activity, its machine and age,
  // core's summary of the exact operation, that operation's inputs with long
  // ones folded, and only the answers core accepts. Core applies the first
  // answer from any device. A later one comes back as already answered and
  // its line shows here.
  import Button from '../lib/ui/Button.svelte'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import MachineMark from './MachineMark.svelte'
  import { agentRequest } from './agent-runtime.js'
  import { approvalChoices, approvalOutcome, approvalSummary, longInput } from './approvals.js'
  import { whenLabel } from '../lib/agent-labels.js'

  let { tauri, approval, agent = null, activity = undefined, machine = null, inline = false, onanswered = () => {}, onopen = null } = $props()
  let busy = $state(false)
  let note = $state('')
  let settled = $state(false)
  let open = $state(false)
  const summary = $derived(approvalSummary(approval))
  const choices = $derived(approvalChoices(approval))
  const folds = $derived(summary.rows.some(([, value]) => longInput(value)))
  const shown = $derived(agent ?? { id: approval.agent_id, name: approval.agent_name })

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
  <p class="operation">{summary.operation}</p>
  {#if summary.rows.length}
    <dl class="inputs" class:folded={folds && !open}>
      {#each summary.rows as [label, value] (label)}<div><dt>{label}</dt><dd>{value}</dd></div>{/each}
    </dl>
    {#if folds}<button type="button" class="more" aria-expanded={open} onclick={() => { open = !open }}>{open ? 'Show less' : 'Show all'}</button>{/if}
  {/if}
  {#if !settled}
    <div class="actions">
      {#each choices as choice (choice.label)}
        <Button variant={choice.tone === 'deny' ? 'outline' : 'default'} disabled={busy} onclick={() => answer(choice)}>{choice.label}</Button>
      {:else}
        <p class="note">This request needs its conversation.</p>
      {/each}
      {#if onopen}<Button variant="outline" disabled={busy} onclick={() => onopen(approval)}>Open conversation</Button>{/if}
    </div>
  {/if}
  {#if note}<p class="note" role="status">{note}</p>{/if}
</article>

<style>
  .approval { display: grid; gap: 8px; padding: 12px 0; min-width: 0; }
  .approval.inline { padding: 12px 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  .who { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; font-size: var(--text-13); }
  .when { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .operation { margin: 0; font: var(--text-13) var(--font-mono); overflow-wrap: anywhere; }
  .inputs { display: grid; gap: 4px; margin: 0; padding: 8px 10px; border-radius: var(--radius-control); background: var(--faint); font: var(--text-12) var(--font-mono); }
  .inputs.folded { max-height: 7.5em; overflow: hidden; mask-image: linear-gradient(to bottom, currentColor 70%, transparent); }
  .inputs div { display: grid; grid-template-columns: 72px minmax(0, 1fr); gap: 8px; }
  dt { color: var(--muted); }
  dd { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; }
  .more { justify-self: start; min-height: 24px; padding: 2px 6px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: inherit; font-size: var(--text-12); cursor: pointer; }
  .more:hover { background: var(--faint); color: var(--ink); }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .note { margin: 0; color: var(--muted); font-size: var(--text-12); }
</style>
