<script>
  // Creates a group chat of two to six agents from any machine. In meeting
  // mode the first round is blind, debate rounds follow, and the lead
  // summarizes. Core runs the group on the machine that creates it.
  import AgentDialog from './AgentDialog.svelte'
  import Button from '../lib/ui/Button.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import MachineMark from './MachineMark.svelte'
  import { agentRequest } from './agent-runtime.js'
  import { MAX_DEBATE_ROUNDS, MAX_MEMBERS, groupRequest, groupRuntime } from './groups.js'

  let { tauri, agents = [], swarm = null, onclose, oncreated = () => {} } = $props()
  let name = $state('')
  let members = $state([])
  let lead = $state('')
  let meetingMode = $state(false)
  let debateRounds = $state(MAX_DEBATE_ROUNDS)
  let busy = $state(false)
  let error = $state('')
  const runtime = $derived(groupRuntime(swarm))
  const draft = $derived(groupRequest({ name, members, lead, meetingMode, debateRounds }))

  function choose(id, on) {
    members = on ? [...members, id] : members.filter((entry) => entry !== id)
    if (!members.includes(lead)) lead = members[0] ?? ''
  }

  async function create() {
    if (draft.error || !runtime) { error = draft.error ?? 'Link this computer into a swarm first.'; return }
    busy = true
    error = ''
    try {
      const answer = await agentRequest(tauri, runtime, 'group.create', draft.body)
      oncreated({ ...answer.group, runtimeId: runtime })
      onclose()
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }
</script>

<AgentDialog title="New group" panel="group" {busy} {onclose}>
  <label>Name<input bind:value={name} maxlength="80" placeholder="Launch room" disabled={busy} /></label>
  <fieldset>
    <legend>Agents <span class="count">{members.length} of {MAX_MEMBERS}</span></legend>
    <ul class="members">
      {#each agents as agent (agent.id)}
        <li><label class="check"><input type="checkbox" checked={members.includes(agent.id)} disabled={busy || (!members.includes(agent.id) && members.length >= MAX_MEMBERS)} onchange={(event) => choose(agent.id, event.currentTarget.checked)} /><AgentAvatar {agent} size={20} /><span class="name">{agent.name}</span>{#if agent.primaryMachine}<MachineMark machine={agent.primaryMachine} />{/if}</label></li>
      {:else}
        <li class="note">No agents yet.</li>
      {/each}
    </ul>
  </fieldset>
  <label>Lead<select bind:value={lead} disabled={busy || members.length === 0}>{#each members as id (id)}<option value={id}>{agents.find((agent) => agent.id === id)?.name}</option>{/each}</select></label>
  <div class="line"><Toggle bind:checked={meetingMode} label="Meeting mode" disabled={busy} /><span>Meeting mode</span></div>
  {#if meetingMode}
    <p class="note">Each agent answers alone first, then reads the others and responds. The lead writes the summary.</p>
    <label>Debate rounds<select bind:value={debateRounds} disabled={busy}>{#each Array.from({ length: MAX_DEBATE_ROUNDS + 1 }, (_, index) => index) as rounds (rounds)}<option value={rounds}>{rounds}</option>{/each}</select></label>
  {:else}
    <p class="note">Mention agents with @name or @everyone. With no mention, the first agent answers.</p>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#snippet actions()}
    <span class="spacer"></span>
    <Button disabled={busy} onclick={onclose}>Cancel</Button>
    <Button variant="primary" disabled={busy || !!draft.error} onclick={create}>{busy ? 'Creating…' : 'Create group'}</Button>
  {/snippet}
</AgentDialog>

<style>
  label { display: grid; gap: 4px; color: var(--muted); font-size: var(--text-12); }
  input:not([type="checkbox"]), select { box-sizing: border-box; width: 100%; min-height: 32px; padding: 5px 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; font-size: var(--text-13); }
  fieldset { display: grid; gap: 6px; margin: 0; padding: 0; border: 0; }
  legend { margin-bottom: 4px; color: var(--muted); font-size: var(--text-12); }
  .count { font-family: var(--font-mono); }
  .members { display: grid; gap: 2px; max-height: 240px; overflow-y: auto; margin: 0; padding: 0; list-style: none; }
  .check { display: flex; align-items: center; gap: 8px; min-height: 28px; color: var(--ink); font-size: var(--text-13); }
  .name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .line { display: flex; align-items: center; gap: 8px; font-size: var(--text-13); }
  .note { margin: 0; color: var(--muted); font-size: var(--text-12); line-height: 1.5; }
  .error { margin: 0; color: var(--oxide); font-size: var(--text-12); }
  .spacer { flex: 1; }
</style>
