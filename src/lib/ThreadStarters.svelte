<script>
  // A new thread's empty state: one starter card per common task, each icon in
  // its own starter tint. `onpick` starts the starter's flow.
  import LucideIcon from './LucideIcon.svelte'
  import { threadStarters } from './creation-prompts.js'
  let { disabled = false, onpick } = $props()
</script>

<section class="thread-starters" aria-label="Start with">
  <ul>
    {#each threadStarters as starter (starter.id)}
      <li>
        <button type="button" class="starter" style:--tint={`var(--tint-${starter.tint})`} {disabled} onclick={() => onpick?.(starter)}>
          <span class="icon" aria-hidden="true"><LucideIcon name={starter.icon} size={18} variant="action" /></span>
          <span class="name">{starter.name}</span>
        </button>
      </li>
    {/each}
  </ul>
</section>

<style>
  .thread-starters { display: flex; justify-content: center; margin-top: 18vh; padding: 0 16px; }
  ul { display: grid; grid-template-columns: repeat(2, minmax(0, 220px)); gap: 10px; margin: 0; padding: 0; list-style: none; }
  .starter { display: flex; align-items: center; gap: 10px; width: 100%; min-height: 52px; padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); color: var(--ink); font: var(--weight-semibold) var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .starter:hover { background: var(--faint); }
  .starter:focus-visible { border-color: var(--ink); outline: 0; }
  .starter:disabled { cursor: default; opacity: .55; }
  .icon { display: inline-flex; flex: none; align-items: center; justify-content: center; width: 32px; height: 32px; border-radius: var(--radius-control); background: color-mix(in oklab, var(--tint) 14%, var(--surface)); color: var(--tint); }
  .name { min-width: 0; }
  @media (max-width: 520px) { ul { grid-template-columns: minmax(0, 1fr); } }
</style>
