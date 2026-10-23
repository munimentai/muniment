<script>
  // Agent approvals waiting on any member of the swarm, each as an approval
  // card. `inline` draws them as cards in an agent's thread. The line an
  // answer earns stays under the list after its card leaves.
  import ApprovalCard from './ApprovalCard.svelte'
  import { agentActivity } from './activity.js'
  import { memberMachine } from './swarm.js'

  let { tauri, approvals = [], agents = [], swarm = null, runtimeList = null, live = {}, inline = false, empty = 'No approvals waiting.', onanswered = () => {}, onopen = null } = $props()
  let notice = $state(null)
  const shown = $derived(notice && !approvals.some((approval) => approval.approval_id === notice.id) ? notice.text : '')
</script>

{#if approvals.length || !inline}
  <ul class="approvals" class:inline aria-label="Waiting for approval">
    {#each approvals as approval (approval.approval_id)}
      {@const agent = agents.find((entry) => entry.id === approval.agent_id) ?? null}
      <li>
        <ApprovalCard {tauri} {approval} {agent} {inline} activity={agentActivity(agent ?? { id: approval.agent_id }, live, swarm?.agents)}
          machine={inline ? null : memberMachine(swarm, approval.runtime_id, runtimeList)} {onopen}
          onanswered={(answered, result, line) => { notice = line ? { id: answered.approval_id, text: line } : null; onanswered(answered, result, line) }} />
      </li>
    {:else}
      <li class="empty">{empty}</li>
    {/each}
  </ul>
{/if}
{#if shown}<p class="note" role="status">{shown}</p>{/if}

<style>
  .approvals { display: grid; gap: 0; margin: 0; padding: 0; list-style: none; }
  .approvals:not(.inline) li + li { border-top: 1px solid var(--border); }
  .approvals.inline { gap: 8px; }
  .note, .empty { margin: 0; color: var(--muted); font-size: var(--text-12); }
  .empty { padding: 12px 0; font-size: var(--text-13); }
</style>
