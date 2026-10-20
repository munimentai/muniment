<script>
  // Moves an agent's primary machine. The machine that holds the agent sends
  // the move, and core hands over its definition, memory, routines and
  // threads. When the primary does not answer, a forced move installs the
  // agent on the target, and the old primary steps down when it returns.
  import AgentDialog from './AgentDialog.svelte'
  import Button from '../lib/ui/Button.svelte'
  import MachineMark from './MachineMark.svelte'
  import { agentRequest } from './agent-runtime.js'
  import { memberMachine, moveOutcome, moveRequest } from './swarm.js'

  // `destination` is the machine the move starts on, such as one picked in the profile.
  let { tauri, agent, swarm, runtimeList = null, destination = '', onclose, onmoved = () => {} } = $props()
  const choices = $derived((swarm?.members ?? []).filter((member) => member.runtime_id !== agent.primaryRuntime))
  let to = $state('')
  let force = $state(false)
  let confirmed = $state(false)
  let busy = $state(false)
  let error = $state('')
  const current = $derived(memberMachine(swarm, agent.primaryRuntime, runtimeList))
  const target = $derived(to ? memberMachine(swarm, to, runtimeList) : null)
  const plan = $derived(to ? moveRequest({ agent, to, swarm, force }) : null)

  $effect(() => { if (!to && choices.length) to = choices.find((member) => member.runtime_id === destination)?.runtime_id ?? choices[0].runtime_id })

  async function move() {
    if (!plan?.runtime) return
    busy = true
    error = ''
    try {
      const answer = await agentRequest(tauri, plan.runtime, 'agent.move', plan.body)
      onmoved(moveOutcome(answer?.status, target?.name ?? 'the machine'))
      onclose()
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }
</script>

<AgentDialog title="Move agent" panel="move" {busy} width={480} {onclose}>
  <p class="lead">Move {agent.name} and its memory, routines and threads to another machine. Routines then run there.</p>
  <div class="from"><span>Now on</span><MachineMark machine={current} />{#if !current.online}<span class="offline">Offline</span>{/if}</div>
  {#if choices.length}
    <label>Move to<select bind:value={to} disabled={busy}>{#each choices as member (member.runtime_id)}<option value={member.runtime_id}>{memberMachine(swarm, member.runtime_id, runtimeList).name}{member.online ? '' : ' (offline)'}</option>{/each}</select></label>
  {:else}
    <p class="note">Add a machine to the fleet in Settings, Machines, to move this agent.</p>
  {/if}
  {#if plan && !plan.primaryOnline}
    <div class="warning" role="note">
      <p>{current.name} does not answer. A forced move starts {agent.name} on {target?.name} without its latest work from {current.name}. When {current.name} returns, it stops running this agent.</p>
      <label class="check"><input type="checkbox" bind:checked={force} disabled={busy} />Force the move</label>
    </div>
  {/if}
  {#if plan?.error && (plan.primaryOnline || force)}<p class="error" role="alert">{plan.error}</p>{/if}
  {#if plan?.runtime}<label class="check"><input type="checkbox" bind:checked={confirmed} disabled={busy} />Move {agent.name} to {target?.name}</label>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#snippet actions()}
    <span class="spacer"></span>
    <Button disabled={busy} onclick={onclose}>Cancel</Button>
    <Button variant="primary" disabled={busy || !plan?.runtime || !confirmed} onclick={move}>{busy ? 'Moving…' : force ? 'Force move' : 'Move agent'}</Button>
  {/snippet}
</AgentDialog>

<style>
  .lead, .note { margin: 0; font-size: var(--text-13); line-height: 1.5; }
  .note { color: var(--muted); }
  .from { display: flex; align-items: center; gap: 8px; color: var(--muted); font-size: var(--text-12); }
  .offline { color: var(--ochre); font: var(--text-12) var(--font-mono); }
  label { display: grid; gap: 4px; color: var(--muted); font-size: var(--text-12); }
  .check { display: flex; align-items: center; gap: 8px; color: var(--ink); font-size: var(--text-13); }
  select { min-height: 32px; padding: 5px 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; font-size: var(--text-13); }
  .warning { display: grid; gap: 8px; padding: 10px; border: 1px solid var(--ochre); border-radius: var(--radius-control); }
  .warning p { margin: 0; font-size: var(--text-13); line-height: 1.5; }
  .error { margin: 0; color: var(--oxide); font-size: var(--text-12); }
  .spacer { flex: 1; }
</style>
