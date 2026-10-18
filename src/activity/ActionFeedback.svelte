<script>
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ToolCall from './ToolCall.svelte'
  import { actionGroups, actionDuration, outputLimitNote } from './action-feedback.js'

  let { activities = [], live = false, onopenfile, loadoutput } = $props()
  // The whole output of a truncated action, read when its detail first opens.
  let outputs = $state({})
  function readOutput(action, retry = false) {
    if (!loadoutput || !action.outputTruncated || (outputs[action.effectId] && !retry)) return
    outputs[action.effectId] = loadoutput(action.effectId)
  }
  const groups = $derived(actionGroups(activities, live))
  // A script counts as the calls it made, so the count names the calls a person asked for.
  const total = $derived(groups.reduce((sum, group) => sum + group.calls, 0))
  const running = $derived(groups.some((group) => group.running))
  let now = $state(Date.now())
  $effect(() => {
    if (!live) return
    const timer = setInterval(() => { if (live) now = Date.now() }, 1000)
    return () => clearInterval(timer)
  })
</script>

{#snippet line(action, via = '')}
  <ToolCall presentation={action.presentation} label={action.label} running={action.running} result={action.result} failed={action.failed} state={action.state} {via} />{#if action.counts}<span class="counts">{action.counts}</span>{/if}
{/snippet}

<!-- The state, details and output of one call, the raw record behind Show details. -->
{#snippet detail(action)}
  {#snippet output()}
    {#if action.output}<strong>Output</strong>
      {#if outputs[action.effectId]}
        {#await outputs[action.effectId]}
          <pre>{action.output}</pre><p>Loading the full output.</p>
        {:then full}
          <pre>{full.text}</pre>{#if full.truncated}<p>{outputLimitNote(full)}</p>{/if}
        {:catch}
          <pre>{action.output}</pre><p role="alert">The full output could not be loaded. <button type="button" onclick={() => readOutput(action, true)}>Load full output</button></p>
        {/await}
      {:else}<pre>{action.output}</pre>{/if}
    {:else}<p>{action.running ? 'Waiting for output.' : 'No output was saved.'}</p>{/if}
  {/snippet}
  <ToolCall variant="detail" presentation={action.presentation} state={`${action.state}${action.startedAt && (action.finishedAt || action.running) ? ` · ${actionDuration(action.startedAt, action.finishedAt || (action.running ? now : action.startedAt))}` : ''}`} input={action.detailInput} raw={output} />
{/snippet}

<!-- One row. A script's calls nest under its row, and a script that made one call shows as that call, with the script in its detail. -->
{#snippet actionRow(action)}
  {#if action.path && onopenfile && !action.script}
    <button type="button" class="file-action" aria-label={action.label.replaceAll('`', '')} onclick={() => onopenfile({ path: action.path })}>{@render line(action)}</button>
  {:else if !action.hasDetails && !action.script}
    <div class="plain-action">{@render line(action)}</div>
  {:else}
    <details ontoggle={(event) => { if (event.currentTarget.open) { readOutput(action); if (action.script) readOutput(action.script) } }}>
      <summary>
        {@render line(action, action.via)}
        <span class="chevron"><LucideIcon name="chevron-right" /></span>
      </summary>
      <div class="action-detail">
        {@render detail(action)}
        {#if action.script}
          <div class="action-script">
            <p class="script-line"><ToolCall presentation={action.script.presentation} label={action.script.label} result={action.script.result} failed={action.script.failed} state={action.script.state} iconSize={14} /></p>
            {@render detail(action.script)}
          </div>
        {/if}
      </div>
    </details>
  {/if}
  {#if action.children}
    <div class="action-list nested">
      {#each action.children as child (child.effectId)}{@render actionRow(child)}{/each}
    </div>
  {/if}
{/snippet}

{#snippet renderActions(group)}
  <div class="action-list" class:grouped={group.actions.length > 1}>
    {#each group.actions as action (action.effectId)}{@render actionRow(action)}{/each}
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
  .file-action :global(.tool-label) { text-decoration: underline dotted; text-underline-offset: 3px; }
  .file-action:hover { color: var(--ink); }
  .chevron { flex: none; display: inline-flex; }
  .counts { flex: none; font: var(--text-12) var(--font-mono); }
  .tools-used { width: max-content; max-width: 100%; }
  .tool-groups { padding-left: 14px; }
  details[open] > summary > .chevron { transform: rotate(90deg); }
  .action-list.grouped, .action-list.nested { padding-left: 12px; }
  .action-script { margin-top: 12px; padding-top: 12px; border-top: 1px solid var(--border); }
  .script-line { display: flex; min-height: 24px; align-items: center; }
  .action-detail { margin: 6px 0 12px 24px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--faint); }
  .action-detail p { margin: 0 0 8px; }
  .action-detail strong { font-weight: 500; }
  .action-detail button { min-height: 24px; padding: 2px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; cursor: pointer; }
  pre { font-family: var(--font-mono); font-size: var(--text-13); white-space: pre-wrap; overflow-wrap: anywhere; max-height: 320px; overflow: auto; margin: 8px 0 12px; }
  .active-sheen { background: linear-gradient(100deg, var(--muted) 35%, var(--ink) 50%, var(--muted) 65%); background-size: 250% 100%; background-clip: text; -webkit-background-clip: text; color: transparent; animation: action-sheen 2.4s linear infinite; }
  @keyframes action-sheen { from { background-position: 140% 0; } to { background-position: -40% 0; } }
  :global(:root[data-motion="reduce"]) .active-sheen { animation: none; background: none; color: var(--muted); }
  @media (forced-colors: active) { .active-sheen { background: none; color: CanvasText; } }
</style>
