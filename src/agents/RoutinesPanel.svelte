<script>
  // An agent's routines: proposals from its instructions to accept or
  // dismiss, then each routine with its trigger, last result and switch.
  // Every change is a routine operation on the agent's machine.
  import Toggle from '../lib/Toggle.svelte'
  import Button from '../lib/ui/Button.svelte'
  import RoutineEditor from './RoutineEditor.svelte'
  import { agentRequest } from './agent-runtime.js'
  import { conditionLabel } from './condition.js'
  import { isProposal, lastFiredLabel, triggerSummary } from './routine-labels.js'

  let { agent, tauri, servers = [], webhooks = false, disabled = false, onchange = async () => {} } = $props()
  let editing = $state(null)
  let busy = $state(false)
  let error = $state('')

  const proposals = $derived((agent.routines ?? []).filter(isProposal))
  const routines = $derived((agent.routines ?? []).filter((routine) => !isProposal(routine)))

  async function work(task) {
    busy = true
    error = ''
    try { await task(); await onchange() } catch (failure) { error = String(failure?.message ?? failure) } finally { busy = false }
  }
  const save = (routine) => agentRequest(tauri, agent.runtimeId, 'routine.save', { agent_id: agent.id, routine })
  const remove = (routine) => agentRequest(tauri, agent.runtimeId, 'routine.delete', { agent_id: agent.id, routine_id: routine.id })
  async function saveAndRefresh(routine) { await save(routine); await onchange() }
  async function removeAndRefresh(routine) { await remove(routine); await onchange() }
</script>

<section class="routines" aria-label="Routines">
  <div class="heading"><h3>Routines</h3><Button icon="plus" disabled={disabled || busy} onclick={() => { editing = { routine: null } }}>Add routine</Button></div>
  {#each proposals as routine (routine.id)}
    <article class="proposal" aria-label={`Proposed routine ${routine.name}`}>
      <p class="kicker">Proposed from instructions</p>
      <strong>{routine.name}</strong>
      <p class="record">{triggerSummary(routine.trigger)}</p>
      {#if routine.trigger?.kind === 'mcp' && routine.trigger.condition}<p class="record">When {conditionLabel(routine.trigger.condition)}</p>{/if}
      {#if routine.prompt}<p class="prompt">{routine.prompt}</p>{/if}
      <div class="actions">
        <Button disabled={disabled || busy} onclick={() => work(() => save({ ...routine, enabled: true }))}>Accept</Button>
        <Button disabled={disabled || busy} onclick={() => { editing = { routine } }}>Edit</Button>
        <Button disabled={disabled || busy} onclick={() => work(() => remove(routine))}>Dismiss</Button>
      </div>
    </article>
  {/each}
  {#each routines as routine (routine.id)}
    <div class="routine" role="group" aria-label={routine.name}>
      <div class="text">
        <button type="button" class="name" disabled={disabled || busy} onclick={() => { editing = { routine } }}>{routine.name}</button>
        <span class="record">{triggerSummary(routine.trigger)}</span>
        <span class="record">{lastFiredLabel(routine)}</span>
      </div>
      <Toggle checked={routine.enabled} label={`${routine.name} on`} disabled={disabled || busy} onchange={(checked) => work(() => save({ ...routine, enabled: checked }))} />
    </div>
  {:else}
    {#if !proposals.length}<p class="empty">Runs only when you ask.</p>{/if}
  {/each}
  {#if agent.paused_reason}<p class="note">Routines are paused while no one uses this machine. Open the agent to resume them.</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>

{#if editing}
  <RoutineEditor routine={editing.routine} {servers} {webhooks} onsave={saveAndRefresh} ondelete={editing.routine ? removeAndRefresh : null} onclose={() => { editing = null }} />
{/if}

<style>
  .routines { display: grid; gap: 8px; padding: 12px 0; border-bottom: 1px solid var(--border); }
  .heading { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  h3 { margin: 0; color: var(--muted); font-size: var(--text-12); font-weight: 500; }
  .proposal { display: grid; gap: 4px; padding: 10px; border: 1px dashed var(--border); border-radius: var(--radius-control); }
  .proposal strong { font-size: var(--text-13); }
  .kicker { margin: 0; color: var(--muted); font-size: var(--text-12); }
  .prompt { margin: 0; font-size: var(--text-12); line-height: 1.5; overflow-wrap: anywhere; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 4px; }
  .routine { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .text { display: grid; gap: 2px; min-width: 0; }
  .name { justify-self: start; max-width: 100%; min-height: 24px; padding: 0; border: 0; background: none; color: var(--ink); font: inherit; font-size: var(--text-13); font-weight: 600; text-align: left; overflow-wrap: anywhere; cursor: pointer; }
  .name:hover:not(:disabled) { text-decoration: underline; text-underline-offset: 2px; }
  .record { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .empty, .note { margin: 0; color: var(--muted); font-size: var(--text-12); }
  [role="alert"] { margin: 0; color: var(--oxide); font-size: var(--text-12); }
</style>
