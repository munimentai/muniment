<script>
  // Agent approvals waiting on any member of the swarm: the agent, its
  // machine, the operation and its inputs, with Allow once, Always allow and
  // Deny. Core applies the first answer from any device and reports a later
  // one as already answered.
  import Button from '../lib/ui/Button.svelte'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import MachineMark from './MachineMark.svelte'
  import { agentRequest } from './agent-runtime.js'
  import { approvalChoices, approvalOutcome, approvalSummary } from './approvals.js'
  import { memberMachine } from './swarm.js'
  import { whenLabel } from '../lib/agent-labels.js'

  let { tauri, approvals = [], agents = [], swarm = null, runtimeList = null, onanswered = () => {}, onopen = null } = $props()
  let busy = $state('')
  let notes = $state({})

  async function answer(approval, choice) {
    busy = approval.approval_id
    notes = { ...notes, [approval.approval_id]: '' }
    try {
      const result = await agentRequest(tauri, approval.runtimeId, 'agent.approval_answer', { approval_id: approval.approval_id, answer: choice.answer })
      const line = approvalOutcome(result?.status)
      notes = { ...notes, [approval.approval_id]: line }
      onanswered(approval, result, line ? `${approval.agent_name}: ${line}` : '')
    } catch (failure) {
      notes = { ...notes, [approval.approval_id]: String(failure?.message ?? failure) }
    } finally {
      busy = ''
    }
  }
</script>

<ul class="approvals" aria-label="Waiting for approval">
  {#each approvals as approval (approval.approval_id)}
    {@const summary = approvalSummary(approval)}
    {@const agent = agents.find((entry) => entry.id === approval.agent_id) ?? { id: approval.agent_id, name: approval.agent_name }}
    {@const choices = approvalChoices(approval.request)}
    <li class="approval" aria-label={`Approval for ${approval.agent_name}`}>
      <div class="who">
        <AgentAvatar {agent} size={20} /><strong>{approval.agent_name}</strong>
        <MachineMark machine={memberMachine(swarm, approval.runtime_id, runtimeList)} />
        <span class="when">{whenLabel(approval.created_at)}</span>
      </div>
      <p class="operation">{summary.title}</p>
      {#if summary.inputs}<pre class="inputs">{summary.inputs}</pre>{/if}
      <div class="actions">
        {#each choices as choice (choice.label)}
          <Button variant={choice.tone === 'deny' ? 'outline' : 'default'} disabled={!!busy} onclick={() => answer(approval, choice)}>{choice.label}</Button>
        {:else}
          <p class="note">This request needs its conversation.</p>
        {/each}
        {#if onopen}<Button variant="outline" disabled={!!busy} onclick={() => onopen(approval)}>Open conversation</Button>{/if}
      </div>
      {#if notes[approval.approval_id]}<p class="note" role="status">{notes[approval.approval_id]}</p>{/if}
    </li>
  {:else}
    <li class="empty">No approvals waiting.</li>
  {/each}
</ul>

<style>
  .approvals { display: grid; gap: 0; margin: 0; padding: 0; list-style: none; }
  .approval { display: grid; gap: 6px; padding: 12px 0; }
  .approval + .approval { border-top: 1px solid var(--border); }
  .who { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; font-size: var(--text-13); }
  .when { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .operation { margin: 0; font-size: var(--text-13); overflow-wrap: anywhere; }
  .inputs { margin: 0; padding: 8px; border-radius: var(--radius-control); background: var(--faint); font: var(--text-12) var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .note, .empty { margin: 0; color: var(--muted); font-size: var(--text-12); }
  .empty { padding: 12px 0; font-size: var(--text-13); }
</style>
