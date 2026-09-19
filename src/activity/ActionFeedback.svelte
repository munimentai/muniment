<script>
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { actionGroups, actionDuration } from './action-feedback.js'

  let { activities = [], live = false, onopenfile } = $props()
  const groups = $derived(actionGroups(activities, live))
  let now = $state(Date.now())
  $effect(() => {
    if (!live) return
    const timer = setInterval(() => { if (live) now = Date.now() }, 1000)
    return () => clearInterval(timer)
  })
</script>

{#if groups.length}
  <div class="action-feedback" aria-label="Actions">
    {#each groups as group (group.id)}
      <details >
        <summary>
          <LucideIcon name={group.icon} />
          <span class:active-sheen={group.running}>{group.label}</span>
          <span class="chevron"><LucideIcon name="chevron-right" /></span>
        </summary>
        <div class="action-list">
          {#each group.actions as action (action.effectId)}
            {#if action.path && onopenfile}
              <button type="button" class="file-action" onclick={() => onopenfile({ path: action.path })}>
                <LucideIcon name={action.icon} /><span class="action-label" class:active-sheen={action.running}>{action.label}</span>
                {#if action.state === 'Failed' || action.state === 'Interrupted'}<span>{action.state}</span>{/if}
              </button>
            {:else if !action.hasDetails}
              <div class="plain-action"><LucideIcon name={action.icon} /><span class="action-label" class:active-sheen={action.running}>{action.label}</span>{#if action.state === 'Failed' || action.state === 'Interrupted'}<span>{action.state}</span>{/if}</div>
            {:else}
            <details >
              <summary>
                <LucideIcon name={action.icon} />
                <span class="action-label" class:active-sheen={action.running}>{action.label}</span>
                {#if action.state === 'Failed' || action.state === 'Interrupted'}<span>{action.state}</span>{/if}
                <span class="chevron"><LucideIcon name="chevron-right" /></span>
              </summary>
              <div class="action-detail">
                <p>{action.state}{#if action.startedAt && (action.finishedAt || action.running)} · {actionDuration(action.startedAt, action.finishedAt || (action.running ? now : action.startedAt))}{/if}</p>
                {#if action.input}<strong>{group.kind === 'command' ? 'Command' : 'Input'}</strong><pre>{action.detailInput}</pre>{/if}
                {#if action.output}<strong>Output</strong><pre>{action.output}</pre>
                {:else}<p>{action.running ? 'Waiting for output.' : 'No output was saved.'}</p>{/if}
              </div>
            </details>
            {/if}
          {/each}
        </div>
      </details>
    {/each}
  </div>
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
  .chevron { opacity: 0; flex: none; }
  summary:hover .chevron, summary:focus-visible .chevron, details[open] > summary > .chevron { opacity: 1; }
  details[open] > summary > .chevron { transform: rotate(90deg); }
  .action-list { padding-left: 12px; }
  .action-detail { margin: 6px 0 12px 24px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--faint); }
  .action-detail p { margin: 0 0 8px; }
  .action-detail strong { font-weight: 500; }
  pre { font-family: var(--font-mono); font-size: var(--text-13); white-space: pre-wrap; overflow-wrap: anywhere; max-height: 320px; overflow: auto; margin: 8px 0 12px; }
  .active-sheen { background: linear-gradient(100deg, var(--muted) 35%, var(--ink) 50%, var(--muted) 65%); background-size: 250% 100%; background-clip: text; -webkit-background-clip: text; color: transparent; animation: action-sheen 2.4s linear infinite; }
  @keyframes action-sheen { from { background-position: 140% 0; } to { background-position: -40% 0; } }
  @media (prefers-reduced-motion: reduce) { .active-sheen { animation: none; background: none; color: var(--muted); } }
  @media (hover: none) { .chevron { opacity: 1; } }
  @media (forced-colors: active) { .active-sheen { background: none; color: CanvasText; } }
</style>
