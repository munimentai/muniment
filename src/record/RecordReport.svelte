<script>
  // The company's report: the row shape the sample shows, over the live
  // graph. Every row is one sentence with a magnitude, a claim and a
  // consequence, Review opens the evidence behind it, and a row about one
  // kind opens that kind's table. It reads the runtime and writes nothing.
  import DuplicateReview from './DuplicateReview.svelte'
  import { findingSentence } from './record-sample.js'
  import { reportErrorLine, reportGroups, reportKindLine, reportLine } from './record-report.js'

  let { tauri, company = null, refresh = 0, onopenkind, onmerge } = $props()

  let report = $state(null)
  let error = $state(null)
  let loading = $state(false)
  let open = $state(null)
  let loadVersion = 0
  let reviewVersion = 0
  let comparisons = $state([])
  let reviewError = $state(null)
  let reviewing = $state(false)

  async function review(finding) {
    const version = ++reviewVersion
    if (open === finding.id) { open = null; return }
    open = finding.id
    comparisons = []
    reviewError = null
    reviewing = true
    try {
      const groups = []
      for (const ids of finding.record_groups ?? []) {
        const records = await Promise.all(ids.map(async (id) => {
          const answer = await tauri.invoke('record_entity', { companyId: company.id, entity: id })
          if (answer?.error) throw new Error(answer.error.message)
          if (!answer?.entity?.entity) throw new Error('The record is unavailable. Reopen the report.')
          return answer.entity
        }))
        groups.push(records)
      }
      if (version === reviewVersion) comparisons = groups
    } catch (failure) {
      if (version === reviewVersion) reviewError = reportErrorLine(failure)
    } finally {
      if (version === reviewVersion) reviewing = false
    }
  }

  const groups = $derived(reportGroups(report))
  const headline = $derived(reportLine(company?.name, report))
  const kindLine = $derived(reportKindLine(report?.kinds))

  // Each read carries its version, so a stale answer never lands.
  async function load() {
    if (!tauri || !company) return
    const version = ++loadVersion
    loading = true
    try {
      const answer = await tauri.invoke('record_report', { companyId: company.id })
      if (version !== loadVersion) return
      if (answer?.error) {
        error = reportErrorLine(answer)
        return
      }
      report = answer?.report ?? null
      error = null
    } catch (failure) {
      if (version !== loadVersion) return
      error = reportErrorLine(failure)
    } finally {
      if (version === loadVersion) loading = false
    }
  }

  $effect(() => {
    void company?.id
    void refresh
    reviewVersion += 1
    open = null
    comparisons = []
    void load()
  })
</script>

<section class="record-report" aria-label="Report" data-testid="record-report">
  {#if error}
    <p class="record-report-line" role="alert">{error}</p>
  {:else if !report}
    <p class="record-report-line">{loading ? 'Reading the report' : 'No report yet'}</p>
  {:else}
    <p class="record-report-line" data-testid="record-report-line">{headline}</p>
    {#if groups.length === 0}
      <p class="record-report-empty">No finding is open. Every row resolves to one record.</p>
    {/if}
    <div class="record-report-body">
      {#each groups as group (group.category)}
        <section class="record-report-group" aria-label={group.label}>
          <h4 class="record-report-group-head">{group.label}<span class="record-report-group-count">{group.count}</span></h4>
          <ul class="record-report-rows">
            {#each group.findings as finding (finding.id)}
              <li class="record-report-row">
                <p class="record-report-claim">{findingSentence(finding)}</p>
                <span class="record-report-controls">
                  {#if finding.kind && !finding.record_groups?.length}
                    <button type="button" class="record-report-tool" onclick={() => onopenkind?.(finding.kind)}>Open {finding.kind}</button>
                  {/if}
                  <button type="button" class="record-report-tool" aria-expanded={open === finding.id} onclick={() => review(finding)}>Review</button>
                </span>
                {#if open === finding.id}
                  <div class="record-report-evidence" aria-label="Evidence">
                    {#if reviewing}<p>Reading affected records</p>{/if}
                    {#if reviewError}<p role="alert">{reviewError}</p>{/if}
                    {#each comparisons as records}
                      <DuplicateReview {records} {onmerge} />
                    {/each}
                    <details><summary>Raw evidence</summary>{#each finding.evidence as line (line)}<p class="record-report-evidence-line">{line}</p>{/each}</details>
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
    {#if kindLine}<p class="record-report-foot">{kindLine}</p>{/if}
  {/if}
</section>

<style>
  .record-report { display: grid; grid-template-rows: auto auto minmax(0, 1fr) auto; gap: 10px; min-height: 0; padding-top: 12px; }
  .record-report-line, .record-report-empty { margin: 0; color: var(--ink); font: var(--text-13) var(--font-mono); }
  .record-report-empty { color: var(--muted); font: var(--text-13)/1.5 var(--font-human); }
  .record-report-body { min-height: 0; overflow-y: auto; display: grid; align-content: start; gap: 14px; }
  .record-report-group { display: grid; gap: 6px; }
  .record-report-group-head { display: flex; align-items: baseline; gap: 8px; margin: 0; padding-bottom: 4px; border-bottom: 1px solid var(--border); font: 600 var(--text-13)/1.3 var(--font-human); }
  .record-report-group-count { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .record-report-rows { margin: 0; padding: 0; list-style: none; display: grid; gap: 4px; }
  .record-report-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: baseline; gap: 8px; padding: 6px 0; }
  .record-report-claim { margin: 0; color: var(--ink); font: var(--text-13)/1.5 var(--font-human); }
  .record-report-controls { display: inline-flex; gap: 6px; }
  .record-report-tool { min-height: 24px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); cursor: pointer; white-space: nowrap; }
  .record-report-tool:hover { background: var(--faint); }
  .record-report-evidence { min-width: 0; grid-column: 1 / -1; display: grid; gap: 2px; margin-top: 4px; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--faint); }
  .record-report-evidence-line { margin: 0; color: var(--muted); font: var(--text-12)/1.5 var(--font-mono); overflow-wrap: anywhere; }
  .record-report-foot { margin: 0; color: var(--muted); font: var(--text-12)/1.5 var(--font-mono); }
</style>
