<script>
  import AgentAvatar from './AgentAvatar.svelte'
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
      <li>
        <button type="button" class="starter" onclick={() => oncreate?.(item)}>
          <span class="thumb" aria-hidden="true">
            {#if kind === 'agent'}
              <AgentAvatar agent={{ id: item.id, avatar: { style: 'muniment-v2', seed: `starter-${item.id}` } }} size={compact ? 28 : 40} />
            {:else}
              <svg viewBox="0 0 80 52" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
                {#if item.id === 'report'}
                  <path d="M10 10h34M10 18h40M10 26h28" /><path d="M54 40V30M62 40V22M70 40V34" /><path d="M10 40h30" />
                {:else if item.id === 'dashboard'}
                  <rect x="8" y="8" width="28" height="16" rx="2" /><rect x="44" y="8" width="28" height="16" rx="2" /><path d="M10 44l12-10 10 6 14-12 12 8 12-8" />
                {:else if item.id === 'calculator'}
                  <rect x="22" y="6" width="36" height="10" rx="2" /><path d="M26 26h4M38 26h4M50 26h4M26 35h4M38 35h4M50 35h4M26 44h4M38 44h4M50 44h4" />
                {:else if item.id === 'tracker'}
                  <rect x="10" y="9" width="8" height="8" rx="2" /><path d="M24 13h44" /><rect x="10" y="22" width="8" height="8" rx="2" /><path d="M12 26l2 2 3-4M24 26h36" /><rect x="10" y="35" width="8" height="8" rx="2" /><path d="M24 39h40" />
                {:else}
                  <circle cx="16" cy="13" r="5" /><path d="M28 13h38" /><circle cx="16" cy="27" r="5" /><path d="M28 27h30" /><circle cx="16" cy="41" r="5" /><path d="M28 41h34" />
                {/if}
              </svg>
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
  .compact h3 { color: var(--muted); font: var(--text-12) var(--font-mono); font-weight: 400; }
  .lede { margin: -4px 0 4px; color: var(--muted); font-size: var(--text-13); }
  ul { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 180px), 1fr)); gap: 10px; margin: 0; padding: 0; list-style: none; }
  .compact ul { grid-template-columns: repeat(auto-fill, minmax(min(100%, 150px), 1fr)); gap: 8px; }
  .starter { display: grid; gap: 8px; align-content: start; width: 100%; height: 100%; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: none; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .starter:hover { background: var(--faint); }
  .starter:focus-visible { border-color: var(--ink); outline: 0; }
  .compact .starter { grid-template-columns: auto minmax(0, 1fr); align-items: center; padding: 8px 10px; }
  .thumb { display: flex; align-items: center; justify-content: center; height: 64px; border: 1px solid var(--border); border-radius: var(--radius-control); color: var(--muted); }
  .thumb svg { width: 80px; height: 52px; }
  .compact .thumb { width: 36px; height: 28px; border: 0; }
  .compact .thumb svg { width: 36px; height: 24px; }
  .starter:hover .thumb { color: var(--ink); }
  .name { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; min-width: 0; font-weight: 600; }
  .badge { padding: 0 5px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font: var(--text-12) var(--font-mono); font-weight: 400; }
  .sentence { color: var(--muted); font-size: var(--text-12); }
</style>
