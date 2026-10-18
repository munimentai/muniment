<script>
  // Settings → Data → Retention: how long Muniment keeps completed threads.
  import { onMount } from 'svelte'

  let { tauri } = $props()
  let retentionChoice = $state('keep_every_thread')
  let recordedRetentionChoice = $state('keep_every_thread')
  let retentionSaving = $state(false)
  let retentionError = $state('')
  let retentionRetryChoice = $state(null)
  let retentionLoadRequest = 0
  let retentionSaveRequest = 0

  const retentionOptions = [
    ['Keep every thread', 'keep_every_thread'],
    ['Delete after 30 days', 'delete_after_30_days'],
    ['Delete after 90 days', 'delete_after_90_days'],
    ['Delete after 1 year', 'delete_after_1_year'],
  ]

  async function loadRetentionChoice() {
    const request = ++retentionLoadRequest
    const saveRequest = retentionSaveRequest
    try {
      const choice = await tauri.invoke('thread_retention_choice')
      if (request === retentionLoadRequest && saveRequest === retentionSaveRequest && !retentionSaving) {
        recordedRetentionChoice = choice ?? 'keep_every_thread'
        retentionChoice = recordedRetentionChoice
      }
    } catch (_) {
      if (request === retentionLoadRequest && saveRequest === retentionSaveRequest && !retentionSaving) {
        recordedRetentionChoice = 'keep_every_thread'
        retentionChoice = recordedRetentionChoice
      }
    }
  }

  async function chooseRetention(choice) {
    retentionSaveRequest += 1
    retentionChoice = choice
    retentionSaving = true
    retentionError = ''
    retentionRetryChoice = null
    try {
      await tauri.invoke('record_thread_retention_choice', { choice })
      recordedRetentionChoice = choice
    } catch (_) {
      retentionChoice = recordedRetentionChoice
      retentionError = 'Thread retention could not be saved.'
      retentionRetryChoice = choice
    } finally {
      retentionSaving = false
      retentionSaveRequest += 1
    }
  }

  onMount(loadRetentionChoice)
</script>

<section class="retention-section" aria-labelledby="retention-heading">
  <h3 id="retention-heading" class="settings-heading">Thread retention</h3>
  <p class="section-help">Choose how long Muniment keeps completed threads.</p>
  <div class="retention-options">
    {#each retentionOptions as option}
      <label><input type="radio" name="thread-retention" value={option[1]} checked={retentionChoice === option[1]} disabled={retentionSaving} onchange={() => chooseRetention(option[1])} /> <span>{option[0]}</span></label>
    {/each}
  </div>
  {#if retentionError}<div class="access-status" role="alert"><p>{retentionError}</p><button onclick={() => chooseRetention(retentionRetryChoice)}>Try again</button></div>{/if}
</section>

<style>
  .retention-section { min-width: 0; }
  .retention-section > .settings-heading { margin: 0 0 4px; }
  .section-help { margin: 0 0 10px; color: var(--muted); font-size: var(--text-13); }
  .retention-options { display: grid; padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .retention-options label { display: flex; align-items: center; gap: 8px; min-height: 36px; border-top: 1px solid var(--border); font-size: var(--text-13); cursor: pointer; }
  .retention-options label:first-child { border-top: 0; }
  .retention-options input { min-width: 24px; min-height: 24px; margin: 0; accent-color: var(--ink); cursor: pointer; transform: scale(.67); /* a 24px box drawn at 16px, and its whole row is the label */ }
  .access-status { margin: 8px 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .access-status p { margin: 0 0 6px; }
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
</style>
