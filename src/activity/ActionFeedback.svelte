<script>
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { actionGroups, actionDuration } from './action-feedback.js'

  let { activities = [], live = false, onopenfile } = $props()
  const groups = $derived(actionGroups(activities, live))
  const total = $derived(groups.reduce((sum, group) => sum + group.actions.length, 0))
  const running = $derived(groups.some((group) => group.running))
  let now = $state(Date.now())
  $effect(() => {
    if (!live) return
    const timer = setInterval(() => { if (live) now = Date.now() }, 1000)
    return () => clearInterval(timer)
  })
</script>

{#snippet renderActions(group)}
  <div class="action-list" class:grouped={group.actions.length > 1}>
    {#each group.actions as action (action.effectId)}
      {#if action.path && onopenfile}
        <button type="button" class="file-action" onclick={() => onopenfile({ path: action.path })}>
          <LucideIcon name={action.icon} /><span class="action-label" class:active-sheen={action.running}>{action.label}</span>
          {#if action.state === 'Failed' || action.state === 'Interrupted'}<span>{action.state}</span>{/if}
        </button>
      {:else if !action.hasDetails}
        <div class="plain-action"><LucideIcon name={action.icon} /><span class="action-label" class:active-sheen={action.running}>{action.label}</span>{#if action.state === 'Failed' || action.state === 'Interrupted'}<span>{action.state}</span>{/if}</div>
      {:else}
      <details>
        <summary>
          <LucideIcon name={action.icon} />
          <span class="action-label" class:active-sheen={action.running}>{action.label}</span>
          {#if action.state === 'Failed' || action.state === 'Interrupted'}<span>{action.state}</span>{/if}
          <span class="chevron"><LucideIcon name="chevron-right" /></span>
        </summary>
        <div class="action-detail">
          {#if group.kind === 'web' && action.queries.length}
            <strong>Queries:</strong>
            <ul class="search-queries">{#each action.queries as query}<li>{query}</li>{/each}</ul>
          {:else}
          <p>{action.state}{#if action.startedAt && (action.finishedAt || action.running)} · {actionDuration(action.startedAt, action.finishedAt || (action.running ? now : action.startedAt))}{/if}</p>
          {#if action.input}<strong>{group.kind === 'command' ? 'Command' : 'Input'}</strong><pre>{action.detailInput}</pre>{/if}
          {#if action.output}<strong>Output</strong><pre>{action.output}</pre>
          {:else}<p>{action.running ? 'Waiting for output.' : 'No output was saved.'}</p>{/if}
          {/if}
        </div>
      </details>
      {/if}
    {/each}
  </div>
{/snippet}

{#if groups.length}
  <!-- The tools a reply used fold under one line. It opens to the groups, and each tool opens to its detail. -->
  <details class="action-feedback" aria-label="Actions">
    <summary class="tools-used">
      <span class:active-sheen={running}>{total} {total === 1 ? 'tool' : 'tools'} used</span>
      <span class="chevron"><LucideIcon name="chevron-right" /></span>
    </summary>
    <div class="tool-groups">
    {#each groups as group (group.id)}
      {#if group.actions.length === 1}
        {@render renderActions(group)}
      {:else}
      <details>
        <summary>
          <LucideIcon name={group.icon} />
          <span class:active-sheen={group.running}>{group.label}</span>
          <span class="chevron"><LucideIcon name="chevron-right" /></span>
        </summary>
        {@render renderActions(group)}
      </details>
      {/if}
    {/each}
    </div>
  </details>
{/if}

<style>
  .action-feedback { margin: 12px 0; color: var(--muted); font-family: var(--font-human); }
  summary { display: flex; align-items: center; gap: 8px; min-height: 32px; cursor: pointer; list-style: none; }
  summary::-webkit-details-marker { display: none; }
  summary:hover, summary:focus-visible { color: var(--ink); }
  .file-action, .plain-action { display: flex; align-items: center; gap: 8px; min-height: 32px; min-width: 0; max-width: 100%; }
  .file-action { border: 0; padding: 0; background: transparent; color: inherit; font: inherit; cursor: pointer; }
  .file-action .action-label { text-decoration: underline dotted; text-underline-offset: 3px; }
  .file-action:hover { color: var(--ink); }
  .action-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .chevron { flex: none; display: inline-flex; }
  .tools-used { width: max-content; max-width: 100%; }
  .tool-groups { padding-left: 14px; }
  details[open] > summary > .chevron { transform: rotate(90deg); }
  .action-list.grouped { padding-left: 12px; }
  .action-detail { margin: 6px 0 12px 24px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--faint); }
  .search-queries { list-style: none; margin: 8px 0 0; padding: 0; }
  .search-queries li { margin: 4px 0; overflow-wrap: anywhere; }
  .action-detail p { margin: 0 0 8px; }
  .action-detail strong { font-weight: 500; }
  pre { font-family: var(--font-mono); font-size: var(--text-13); white-space: pre-wrap; overflow-wrap: anywhere; max-height: 320px; overflow: auto; margin: 8px 0 12px; }
  .active-sheen { background: linear-gradient(100deg, var(--muted) 35%, var(--ink) 50%, var(--muted) 65%); background-size: 250% 100%; background-clip: text; -webkit-background-clip: text; color: transparent; animation: action-sheen 2.4s linear infinite; }
  @keyframes action-sheen { from { background-position: 140% 0; } to { background-position: -40% 0; } }
  @media (prefers-reduced-motion: reduce) { .active-sheen { animation: none; background: none; color: var(--muted); } }
  @media (forced-colors: active) { .active-sheen { background: none; color: CanvasText; } }
</style>
