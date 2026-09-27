<script>
  import GraphMark from '../lib/GraphMark.svelte'
  import RecordSources from './RecordSources.svelte'
  import { sourceOptions } from './record-import-state.js'
  import { findingClaim, sampleLeadLine, topFindings } from './record-sample.js'
  import { validCompanyName } from './record-panel-state.js'

  let { name = $bindable(''), source = $bindable(null), oncreate, onsample } = $props()

  const leadLine = sampleLeadLine()
  const leadFindings = topFindings(3)
  const count = (value) => value.toLocaleString('en-US')

  const sourceLabel = $derived(sourceOptions().find((option) => option.value === source)?.label ?? null)
  const submitLabel = $derived(sourceLabel ? `Create your company and connect ${sourceLabel}` : 'Create your company')
</script>

<section class="record-start" aria-label="Start" data-testid="record-start">
  <div class="record-start-graph" aria-hidden="true"><GraphMark size={320} /></div>
  <div class="record-start-column">
    <div class="record-start-plate">
      <h3 class="record-start-headline">Your company, in one record.</h3>
      <p class="record-start-sentence">muniment keeps one record for your company and puts your agent beside it. Every source resolves into that one graph, so the agent answers from what you hold rather than from a copy of it. It proposes every change and you approve it.</p>
    </div>
    <div class="record-start-block">
      <p class="record-start-lead">{leadLine}</p>
      <ul class="record-start-findings">
        {#each leadFindings as finding (finding.id)}
          <li class="record-start-finding">
            <span class="record-start-magnitude">{count(finding.magnitude)}</span>
            <span class="record-start-claim">{findingClaim(finding)}</span>
          </li>
        {/each}
      </ul>
      <button type="button" class="record-start-open" onclick={() => onsample?.()}>Open the sample</button>
    </div>
    <form class="record-start-create" onsubmit={(event) => { event.preventDefault(); oncreate?.() }}>
      <input class="record-start-name" type="text" aria-label="Company name" placeholder="Company name" maxlength="120" bind:value={name}>
      <button type="submit" class="record-start-submit" disabled={!validCompanyName(name)}>{submitLabel}</button>
    </form>
    <div class="record-start-block">
      <h4 class="record-start-title">Read a source in</h4>
      <RecordSources selected={source} onconnect={(value) => { source = source === value ? null : value }} />
      <p class="record-start-foot">Every read runs on this machine, and a credential stays in its keychain.</p>
    </div>
  </div>
</section>

<style>
  .record-start { position: relative; min-height: 0; overflow-x: hidden; overflow-y: auto; padding-top: 14px; }
  .record-start-graph { position: absolute; top: 56%; left: 62%; height: 92%; aspect-ratio: 1; transform: translate(-50%, -50%); pointer-events: none; }
  .record-start-graph { opacity: 0.08; color: var(--ink); }
  .record-start-column { position: relative; display: grid; align-content: start; gap: 22px; max-width: 760px; }
  .record-start-plate { display: grid; gap: 8px; }
  .record-start-headline { margin: 0; font: 600 var(--text-22)/1.3 var(--font-human); }
  .record-start-sentence { margin: 0; max-width: 62ch; color: var(--ink); font: var(--text-13)/1.5 var(--font-human); }
  .record-start-block { display: grid; gap: 10px; }
  .record-start-lead { margin: 0 0 -4px; padding-bottom: 8px; border-bottom: 1px solid var(--border); color: var(--muted); font: var(--text-12)/1.5 var(--font-mono); }
  .record-start-findings { margin: 0; padding: 0; list-style: none; display: grid; }
  .record-start-finding { display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: baseline; gap: 14px; padding: 8px 0; border-bottom: 1px solid var(--border); }
  .record-start-magnitude { color: var(--ink); font: var(--text-28)/1.1 var(--font-mono); font-variant-numeric: tabular-nums; }
  .record-start-claim { color: var(--ink); font: var(--text-13)/1.5 var(--font-human); }
  .record-start-create { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--faint); }
  .record-start-title { margin: 0; font: 600 var(--text-15)/1.3 var(--font-human); }
  .record-start-name { flex: 1 1 220px; min-width: 0; height: 32px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  .record-start-name:focus { outline: none; border-color: var(--muted); }
  .record-start-submit { flex: none; min-height: 32px; padding: 0 14px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--ink); color: var(--paper); font: var(--text-13) var(--font-human); cursor: pointer; }
  .record-start-submit:disabled { background: var(--surface); color: var(--muted); cursor: default; }
  .record-start-open { justify-self: start; min-height: 28px; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); cursor: pointer; }
  .record-start-open:hover { background: var(--faint); }
  .record-start-foot { margin: 0; color: var(--muted); font: var(--text-12)/1.5 var(--font-mono); }
</style>
