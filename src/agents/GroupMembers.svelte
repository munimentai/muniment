<script>
  // The members of the open group, in the composer band where a chat names its
  // model. Each member answers with its own model, so the band shows who is in
  // the group instead. Hover names them all.
  import AgentAvatar from '../lib/AgentAvatar.svelte'

  let { members = [], live = null } = $props()
  const names = $derived(members.map((agent) => agent.name).join(', '))
</script>

<span class="group-members" role="group" aria-label={`Group members: ${names}`} data-tooltip={names}>
  <span class="faces" aria-hidden="true">{#each members.slice(0, 4) as agent (agent.id)}<AgentAvatar {agent} activity={live ? live(agent) : undefined} size={16} />{/each}</span>
  <span>{members.length} {members.length === 1 ? 'agent' : 'agents'}</span>
</span>

<style>
  .group-members { position: relative; display: inline-flex; flex: none; align-items: center; gap: 6px; min-height: 28px; padding: 0 6px; color: var(--muted); font: var(--text-12) var(--font-mono); white-space: nowrap; }
  .group-members::after { content: attr(data-tooltip); position: absolute; left: 0; bottom: calc(100% + 6px); padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  .group-members:hover::after { opacity: 1; transition-delay: 350ms; }
  .faces { display: inline-flex; align-items: center; gap: 2px; }
</style>
