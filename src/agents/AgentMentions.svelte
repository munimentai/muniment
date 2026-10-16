<script>
  // The @ list in a group thread: @everyone, then the members whose name
  // matches. Arrows, Enter, Tab and Escape work as in the file list.
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  let { agents = [], selected = 0, onchoose } = $props()
</script>
<div class="mentions">
  <p>Agents in this group</p>
  {#if !agents.length}<p role="status">No matching agents.</p>{/if}
  <div id="agent-mentions" role="listbox" aria-label="Agent suggestions">
    {#each agents as agent, i (agent.id)}
      <button id={`agent-mention-${i}`} type="button" role="option" aria-selected={selected === i} onpointerdown={(event) => event.preventDefault()} onclick={() => onchoose(agent)}>
        {#if agent.everyone}<LucideIcon name="users" size={16} />{:else}<AgentAvatar {agent} size={16} />{/if}<span>@{agent.name}</span>
      </button>
    {/each}
  </div>
</div>
<style>
  .mentions { position: absolute; bottom: calc(100% + 8px); left: 0; right: 0; max-height: 280px; overflow: auto; padding: 8px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); box-shadow: var(--shadow-overlay); z-index: 8; }
  p { margin: 4px 6px 8px; color: var(--muted); font-size: var(--text-12); }
  button { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 32px; padding: 6px; border: 0; text-align: left; background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); cursor: pointer; }
  button[aria-selected="true"], button:hover { background: var(--faint); }
  span { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
</style>
