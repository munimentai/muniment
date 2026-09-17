<script>
  // The sample company's report: the row shape the local report uses, over a
  // fixture rather than a graph. Every row is one sentence with a magnitude,
  // a claim and a consequence, and Review opens the evidence behind it.
  // Nothing here writes, because the sample lives in a module and not in the
  // record store.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { findingSentence, groupFindings, sampleKindLine, sampleReportLine } from './record-sample.js'

  let { onclose } = $props()
  const groups = groupFindings()
  const reportLine = sampleReportLine()
  const kindLine = sampleKindLine()
  let open = $state(null)
</script>

<section class="record-sample" aria-label="Northwind Traders sample" data-testid="record-sample">
  <div class="record-sample-head">
    <p class="record-sample-line">{reportLine}</p>
    <button type="button" class="record-sample-close" onclick={() => onclose?.()}>
      <LucideIcon name="x" size={14} /><span>Close the sample</span>
    </button>
  </div>
  <div class="record-sample-report">
    {#each groups as group (group.category)}
      <section class="record-sample-group" aria-label={group.label}>
        <h4 class="record-sample-group-head">{group.label}<span class="record-sample-group-count">{group.count}</span></h4>
        <ul class="record-sample-rows">
          {#each group.findings as finding (finding.id)}
            <li class="record-sample-row">
              <p class="record-sample-claim">{findingSentence(finding)}</p>
              <button type="button" class="record-sample-review" aria-expanded={open === finding.id} onclick={() => { open = open === finding.id ? null : finding.id }}>Review</button>
              {#if open === finding.id}
                <div class="record-sample-evidence" aria-label="Evidence">
                  {#each finding.evidence as line (line)}<p class="record-sample-evidence-line">{line}</p>{/each}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
      </section>
    {/each}
  </div>
  <p class="record-sample-foot">{kindLine}</p>
  <p class="record-sample-foot">The sample reads from this window alone. Create your company to accept or deny a finding of your own.</p>
</section>

<style>
  .record-sample { display: grid; grid-template-rows: auto minmax(0, 1fr) auto auto; gap: 10px; min-height: 0; padding-top: 12px; }
  .record-sample-head { display: flex; align-items: center; gap: 12px; }
  .record-sample-line { flex: 1; margin: 0; color: var(--ink); font: var(--text-13) var(--font-mono); }
  .record-sample-close { display: inline-flex; align-items: center; gap: 6px; min-height: 24px; padding: 0 6px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-12) var(--font-mono); cursor: pointer; white-space: nowrap; }
  .record-sample-close:hover { background: var(--faint); }
  .record-sample-report { min-height: 0; overflow-y: auto; display: grid; align-content: start; gap: 14px; }
  .record-sample-group { display: grid; gap: 6px; }
  .record-sample-group-head { display: flex; align-items: baseline; gap: 8px; margin: 0; padding-bottom: 4px; border-bottom: 1px solid var(--border); font: 600 var(--text-13)/1.3 var(--font-human); }
  .record-sample-group-count { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .record-sample-rows { margin: 0; padding: 0; list-style: none; display: grid; gap: 4px; }
  .record-sample-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: baseline; gap: 8px; padding: 6px 0; }
  .record-sample-claim { margin: 0; color: var(--ink); font: var(--text-13)/1.5 var(--font-human); }
  .record-sample-review { min-height: 24px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); cursor: pointer; }
  .record-sample-review:hover { background: var(--faint); }
  .record-sample-evidence { grid-column: 1 / -1; display: grid; gap: 2px; margin-top: 4px; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--faint); }
  .record-sample-evidence-line { margin: 0; color: var(--muted); font: var(--text-12)/1.5 var(--font-mono); overflow-wrap: anywhere; }
  .record-sample-foot { margin: 0; color: var(--muted); font: var(--text-12)/1.5 var(--font-mono); }
</style>
