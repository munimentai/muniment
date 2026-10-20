<script>
  // An agent's routines: proposals from its instructions or an imported
  // template to accept or dismiss, then each routine with its trigger's icon,
  // its name, when it runs, a failed tool check and a small switch. The
  // history lists each run. Every change is a routine operation
  // on the agent's machine. Core reads proposals again after each save, in
  // the background, so the cards reload when the agent changes.
  import Toggle from '../lib/Toggle.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import Button from '../lib/ui/Button.svelte'
  import RoutineEditor from './RoutineEditor.svelte'
  import { agentRequest } from './agent-runtime.js'
  import { conditionLabel } from './condition.js'
  import { isProposal, pollLabel, routineIcon, routineWhen } from './routine-labels.js'

  let { agent, tauri, catalog = null, webhooks = true, disabled = false, onchange = async () => {} } = $props()
  let editing = $state(null)
  let busy = $state(false)
  let error = $state('')
  let proposals = $state([])

  const routines = $derived((agent.routines ?? []).filter((routine) => !isProposal(routine)))

  // The proposals come from core's live operation. The agent's own routines
  // hold them too, so a machine without the operation still shows them.
  async function loadProposals() {
    const current = agent
    if (!current) return
    try {
      const answer = await agentRequest(tauri, current.runtimeId, 'agent.routine_proposals', { agent_id: current.id })
      proposals = answer?.proposals ?? []
    } catch {
      proposals = (current.routines ?? []).filter(isProposal)
    }
  }
  // A save of the instructions changes the agent. Core's model pass adds its
  // proposals a little later, so the cards look again after a pause.
  let revision = 0
  $effect(() => {
    agent.instructions; agent.routines
    const current = ++revision
    void loadProposals()
    const later = setTimeout(() => { if (current === revision) void loadProposals() }, 8000)
    return () => clearTimeout(later)
  })

  async function work(task) {
    busy = true
    error = ''
    try { await task(); await onchange() } catch (failure) { error = String(failure?.message ?? failure) } finally { busy = false }
  }
  const save = (routine) => agentRequest(tauri, agent.runtimeId, 'routine.save', { agent_id: agent.id, routine })
  const remove = (routine) => agentRequest(tauri, agent.runtimeId, 'routine.delete', { agent_id: agent.id, routine_id: routine.id })
  async function saveAndRefresh(routine) { await save(routine); await onchange() }
  async function removeAndRefresh(routine) { await remove(routine); await onchange() }
  const accept = (routine) => agentRequest(tauri, agent.runtimeId, 'agent.routine_proposal_accept', { agent_id: agent.id, routine_id: routine.id })
  const dismiss = (routine) => agentRequest(tauri, agent.runtimeId, 'agent.routine_proposal_dismiss', { agent_id: agent.id, routine_id: routine.id })
  async function createWebhook(routine) {
    const answer = await agentRequest(tauri, agent.runtimeId, 'routine.webhook_create', { agent_id: agent.id, routine })
    await onchange()
    return answer
  }
</script>

<section class="routines" aria-label="Routines">
  <div class="heading"><h3>Routines</h3><Button icon="plus" disabled={disabled || busy} onclick={() => { editing = { routine: null } }}>Add routine</Button></div>
  {#each proposals as routine (routine.id)}
    <article class="proposal" aria-label={`Proposed routine ${routine.name}`}>
      <p class="kicker">{routine.id?.startsWith('import-') ? 'Proposed from the imported template' : 'Proposed from instructions'}</p>
      <strong>{routine.name}</strong>
      <p class="when">{routineWhen(routine)}</p>
      {#if routine.trigger?.kind === 'mcp' && routine.trigger.condition}<p class="record">When {conditionLabel(routine.trigger.condition)}</p>{/if}
      {#if routine.prompt}<p class="prompt">{routine.prompt}</p>{/if}
      <div class="actions">
        <Button disabled={disabled || busy} onclick={() => work(() => accept(routine))}>Accept</Button>
        <Button disabled={disabled || busy} onclick={() => { editing = { routine: { ...routine, enabled: true } } }}>Edit</Button>
        <Button disabled={disabled || busy} onclick={() => work(() => dismiss(routine))}>Dismiss</Button>
      </div>
    </article>
  {/each}
  {#each routines as routine (routine.id)}
    <div class="routine" role="group" aria-label={routine.name}>
      <span class="icon"><LucideIcon name={routineIcon(routine.trigger)} variant="action" size={14} /></span>
      <div class="text">
        <button type="button" class="name" disabled={disabled || busy} onclick={() => { editing = { routine } }}>{routine.name}</button>
        <span class="when">{routineWhen(routine)}</span>
        {#if routine.last_poll?.error}<span class="poll-error">{pollLabel(routine)}</span>{/if}
      </div>
      <Toggle size="small" checked={routine.enabled} label={`${routine.name} on`} disabled={disabled || busy} onchange={(checked) => work(() => save({ ...routine, enabled: checked }))} />
    </div>
  {:else}
    {#if !proposals.length}<p class="empty">No routines. It runs only when you ask.</p>{/if}
  {/each}
  {#if agent.paused_reason}<p class="note">{agent.paused_reason === 'absence' ? 'Routines paused after 14 days without use. Resume them from the sidebar.' : 'Routines are paused.'}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>

{#if editing}
  <RoutineEditor routine={editing.routine} {catalog} {webhooks} {tauri} {agent} onsave={saveAndRefresh} oncreatewebhook={createWebhook} ondelete={editing.routine && !isProposal(editing.routine) ? removeAndRefresh : null} onclose={() => { editing = null }} />
{/if}

<style>
  .routines { display: grid; gap: 6px; padding: 10px 0; border-bottom: 1px solid var(--border); }
  .heading { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  h3 { margin: 0; color: var(--muted); font-size: var(--text-12); font-weight: 500; }
  .proposal { display: grid; gap: 4px; padding: 10px; border: 1px dashed var(--border); border-radius: var(--radius-control); }
  .proposal strong { font-size: var(--text-13); }
  .kicker { margin: 0; color: var(--muted); font-size: var(--text-12); }
  .prompt { margin: 0; font-size: var(--text-12); line-height: 1.5; overflow-wrap: anywhere; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 4px; }
  .routine { display: flex; align-items: flex-start; gap: 8px; }
  .icon { flex: none; display: inline-flex; padding-top: 4px; color: var(--muted); }
  .text { display: grid; flex: 1; gap: 1px; min-width: 0; }
  .routine > :global([role="switch"]) { margin-top: 4px; }
  .name { justify-self: start; max-width: 100%; min-height: 22px; padding: 0; border: 0; background: none; color: var(--ink); font: inherit; font-size: var(--text-13); text-align: left; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; cursor: pointer; }
  .when { margin: 0; color: var(--muted); font: var(--text-12) var(--font-human); }
  .poll-error { color: var(--oxide); font: var(--text-12) var(--font-human); overflow-wrap: anywhere; }
  .name:hover:not(:disabled) { text-decoration: underline; text-underline-offset: 2px; }
  .record { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .empty, .note { margin: 0; color: var(--muted); font-size: var(--text-12); }
  [role="alert"] { margin: 0; color: var(--oxide); font-size: var(--text-12); }
</style>
