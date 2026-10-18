<script>
  import AgentAvatar from './AgentAvatar.svelte'
  import { agentColor } from './agent-avatar.js'
  import LucideIcon from './LucideIcon.svelte'
  // The row of starters at the top of the Artifacts and Agents catalogs. Each
  // tile opens a creation chat with its goal filled in. An empty catalog shows
  // the full tiles with a sentence each; a full one shows them as one compact
  // line above its cards.
  let { heading, lede = '', kind, items = [], compact = false, oncreate } = $props()
</script>

<section class="starters" class:compact aria-label={heading}>
  <h3>{heading}</h3>
  {#if lede && !compact}<p class="lede">{lede}</p>{/if}
  <ul>
    {#each items as item (item.id)}
      {@const starter = kind === 'agent' ? { id: item.id, avatar: { style: 'muniment', seed: `starter-${item.id}` } } : null}
      <li>
        <button type="button" class="starter" class:agent={!!starter} style:--agent-color={starter ? agentColor(starter) : undefined} onclick={() => oncreate?.(item)}>
          <span class="thumb" aria-hidden="true">
            {#if starter}
              <AgentAvatar agent={starter} size={compact ? 28 : 40} />
            {:else}
              <LucideIcon name={item.icon} size={compact ? 20 : 28} />
            {/if}
          </span>
          <span class="name">{item.name}{#if item.badge}<small class="badge">{item.badge}</small>{/if}</span>
          {#if !compact}<span class="sentence">{item.sentence}</span>{/if}
        </button>
      </li>
    {/each}
  </ul>
</section>

<style>
  .starters { display: grid; gap: 10px; min-width: 0; }
  h3 { margin: 0; color: var(--ink); font-size: var(--text-15); font-weight: 600; }
  .compact h3 { color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  .lede { margin: -4px 0 4px; color: var(--muted); font-size: var(--text-13); }
  ul { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 180px), 1fr)); gap: 10px; margin: 0; padding: 0; list-style: none; }
  .compact ul { grid-template-columns: repeat(auto-fill, minmax(min(100%, 150px), 1fr)); gap: 8px; }
  .starter { display: grid; gap: 8px; align-content: start; width: 100%; height: 100%; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: none; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .starter:hover { background: var(--faint); }
  .starter:focus-visible { border-color: var(--ink); outline: 0; }
  .compact .starter { grid-template-columns: auto minmax(0, 1fr); align-items: center; padding: 8px 10px; }
  .thumb { display: flex; align-items: center; justify-content: center; height: 64px; border: 1px solid var(--border); border-radius: var(--radius-control); color: var(--muted); }
  .compact .thumb { width: 36px; height: 36px; border: 0; }
  /* An agent starter sits its head on a tile tinted with the agent's color. */
  .agent .thumb { border-color: transparent; background: color-mix(in oklab, var(--agent-color) 28%, var(--surface)); }
  .agent:hover .thumb { background: color-mix(in oklab, var(--agent-color) 40%, var(--surface)); }
  .starter:hover .thumb { color: var(--ink); }
  .name { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; min-width: 0; font-weight: 600; }
  .badge { padding: 0 5px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font: var(--text-12) var(--font-human); font-weight: 400; }
  .sentence { color: var(--muted); font-size: var(--text-12); }
</style>
