<script>
  import { onMount } from 'svelte'
  import { open } from '@tauri-apps/plugin-dialog'

  import { formatByteSize } from './chat-state.js'
  import { onboardingCancelSettingsState, onboardingConfirmedHomePathState, onboardingConfirmedState, onboardingConfirmingState, onboardingErrorState, onboardingExtractingState, onboardingExtractionErrorState, onboardingExtractionState, onboardingFinalizingState, onboardingImportChoiceState, onboardingImportErrorState, onboardingImportSavedState, onboardingImportSavingState, onboardingPathState, onboardingPreviewErrorState, onboardingPreviewingState, onboardingPreviewState, onboardingReturnToArchiveReviewState, onboardingSelectionState, onboardingStatusState } from './onboarding-state.js'

  let { tauri, onboarding = $bindable() } = $props()
  let onboardingPreviewSequence = 0

  async function loadOnboarding() {
    try {
      onboarding = onboardingStatusState(await tauri.invoke('home_status'))
    } catch (error) {
      onboarding = { name: 'load-error', homePath: '', error: typeof error === 'string' ? error : 'Onboarding could not be loaded.' }
    }
  }

  async function chooseHome() {
    try {
      const picked = await open({ directory: true, multiple: false, defaultPath: onboarding.homePath })
      if (typeof picked === 'string') onboarding = onboardingPathState(onboarding, picked)
    } catch (_) {
      onboarding = onboardingErrorState(onboarding, 'The folder picker could not be opened. Try again.')
    }
  }

  async function confirmHome() {
    const pending = onboardingConfirmingState(onboarding)
    onboarding = pending
    try {
      onboarding = onboardingConfirmedState(pending, await tauri.invoke('home_confirm', { homePath: pending.homePath }))
    } catch (error) {
      onboarding = onboardingErrorState(pending, typeof error === 'string' ? error : undefined)
    }
  }

  async function chooseImportArchive() {
    let picked
    try {
      picked = await open({ multiple: false, directory: false, filters: [{ name: 'ZIP archives', extensions: ['zip'] }] })
    } catch (_) {
      onboarding = { ...onboarding, error: 'The file picker could not be opened. Try again.' }
      return
    }
    if (typeof picked !== 'string') return
    const previewId = ++onboardingPreviewSequence
    const pending = onboardingPreviewingState(onboarding, picked)
    onboarding = pending
    try {
      const manifest = await tauri.invoke('onboarding_import_preview', { archivePath: picked })
      if (previewId === onboardingPreviewSequence) onboarding = onboardingPreviewState(pending, manifest)
    } catch (error) {
      if (previewId === onboardingPreviewSequence) onboarding = onboardingPreviewErrorState(pending, error)
    }
  }

  async function skipImport() {
    onboardingPreviewSequence += 1
    const pending = onboardingFinalizingState(onboarding)
    onboarding = pending
    try {
      onboarding = onboardingConfirmedState(pending, await tauri.invoke('home_confirm', { homePath: pending.homePath }))
    } catch (error) {
      onboarding = onboardingErrorState(pending, typeof error === 'string' ? error : undefined)
    }
  }

  function selectImportEntry(entryName, selected) {
    onboarding = onboardingSelectionState(onboarding, entryName, selected)
  }

  async function extractImportSelection() {
    if (onboarding.name !== 'reviewing' || onboarding.selectedNames.length === 0) return
    const extractionId = ++onboardingPreviewSequence
    const pending = onboardingExtractingState(onboarding)
    onboarding = pending
    try {
      const extracted = await tauri.invoke('onboarding_import_extract', {
        archivePath: pending.archivePath,
        selectedNames: pending.selectedNames,
      })
      if (extractionId === onboardingPreviewSequence) onboarding = onboardingExtractionState(pending, extracted)
    } catch (error) {
      if (extractionId === onboardingPreviewSequence) onboarding = onboardingExtractionErrorState(pending, error)
    }
  }

  function returnToArchiveReview() {
    onboardingPreviewSequence += 1
    onboarding = onboardingReturnToArchiveReviewState(onboarding)
  }

  async function chooseConfirmedHome() {
    try {
      const picked = await open({ directory: true, multiple: false, defaultPath: onboarding.homePath })
      if (typeof picked === 'string') onboarding = onboardingConfirmedHomePathState(onboarding, picked)
    } catch (_) {
      onboarding = { ...onboarding, error: 'The folder picker could not be opened. Try again.' }
    }
  }

  async function saveConfirmedImport() {
    if (onboarding.name !== 'approved-review') return
    const pending = onboardingImportSavingState(onboarding)
    onboarding = pending
    try {
      await tauri.invoke('home_confirm_import', {
        homePath: pending.homePath,
        approvedEntries: pending.extractedEntries,
      })
      onboarding = onboardingImportSavedState(pending)
    } catch (error) {
      onboarding = onboardingImportErrorState(pending, error)
    }
  }

  onMount(() => {
    loadOnboarding()
  })
</script>

{#if onboarding.name !== 'complete'}
<section class="onboarding" aria-labelledby="onboarding-title">
  <header>
    <p class="eyebrow">{onboarding.savedHomePath ? 'Home settings' : 'First-run setup'}</p>
    <h1 id="onboarding-title">{['approved-review', 'import-saving', 'import-invalid'].includes(onboarding.name) ? 'Save approved files' : ['import-choice', 'previewing', 'reviewing', 'extracting', 'finalizing'].includes(onboarding.name) ? 'Review an assistant export' : 'Choose your Muniment Home'}</h1>
  </header>
  <div class="onboarding-content">
  {#if onboarding.name === 'loading'}
    <p class="support" role="status">Finding your Documents folder…</p>
  {:else if ['choosing', 'confirming', 'settings', 'confirming-settings'].includes(onboarding.name)}
    <p class="support">Your memory stays in plain Markdown files in a folder you control. Muniment will create four visible folders inside it.</p>
    <div class="path-card">
      <span class="path-label">Home location</span>
      <strong data-testid="onboarding-home-path">{onboarding.homePath}</strong>
      <button data-testid="onboarding-picker" onclick={chooseHome} disabled={onboarding.name.startsWith('confirming')}>Choose folder…</button>
    </div>
    <p class="folder-preview"><span>memory/</span><span>agents/</span><span>projects/</span><span>sessions/</span></p>
    {#if onboarding.error}<p class="onboarding-error" role="alert">{onboarding.error}</p>{/if}
  {:else if ['import-choice', 'previewing', 'reviewing', 'extracting', 'finalizing'].includes(onboarding.name)}
    <p class="support">Optionally choose one assistant export ZIP. Preview happens locally and is read-only; nothing is imported or sent to a model.</p>
    {#if ['reviewing', 'extracting'].includes(onboarding.name)}
      <div class="manifest-summary">
        <span>Supported files · {onboarding.selectedNames.length} of {onboarding.manifest.entries.length} selected</span>
        <strong>{onboarding.manifest.entries.length} · {formatByteSize(onboarding.manifest.totalByteSize)} expanded</strong>
      </div>
      {#if onboarding.manifest.entries.length}
        <ol class="manifest" aria-label="Export manifest">
          {#each onboarding.manifest.entries as entry}
            <li>
              <label class="manifest-consent"><input type="checkbox" checked={onboarding.selectedNames.includes(entry.name)} onchange={(event) => selectImportEntry(entry.name, event.currentTarget.checked)} disabled={onboarding.name === 'extracting'} /><span class="manifest-meta"><strong>{entry.name}</strong><span>{entry.kind} · {formatByteSize(entry.byteSize)} · {entry.excerptTruncated ? 'excerpt truncated' : 'complete excerpt'}</span></span></label>
              <pre>{entry.excerpt}</pre>
            </li>
          {/each}
        </ol>
      {:else}<p class="empty-manifest">No supported files were found in this ZIP.</p>{/if}
    {:else}
      <div class="path-card">
        <span class="path-label">Assistant export</span>
        <strong>{onboarding.archivePath ?? 'No ZIP selected'}</strong>
        <button data-testid="onboarding-import-picker" onclick={chooseImportArchive} disabled={['previewing', 'finalizing'].includes(onboarding.name)}>{onboarding.name === 'previewing' ? 'Reading archive…' : 'Choose ZIP…'}</button>
      </div>
    {/if}
    {#if onboarding.error}<p class="onboarding-error" role="alert">{onboarding.error}</p>{/if}
  {:else if ['approved-review', 'import-saving', 'import-invalid'].includes(onboarding.name)}
    <p class="support">{onboarding.extractedEntries.length} approved {onboarding.extractedEntries.length === 1 ? 'file is' : 'files are'} ready to save as verbatim originals.</p>
    <ul class="approved-sources" aria-label="Approved sources">{#each onboarding.extractedEntries as entry}<li><strong>{entry.sourceName}</strong><span>{entry.sourceProvenance}</span></li>{/each}</ul>
    <div class="path-card">
      <span class="path-label">Muniment Home</span>
      <strong data-testid="onboarding-home-path">{onboarding.homePath}</strong>
      <button data-testid="onboarding-confirmed-picker" onclick={chooseConfirmedHome} disabled={onboarding.name !== 'approved-review'}>Choose folder…</button>
    </div>
    {#if onboarding.name === 'import-saving'}<p class="support" role="status">Saving your Home and approved files…</p>{/if}
    {#if onboarding.error}
      <div class="onboarding-error" role="alert">
        <p>{onboarding.error}</p>
        {#if onboarding.errorKind === 'destinationConflict' && onboarding.conflictPath}<p>Conflicting destination: <strong>{onboarding.conflictPath}</strong></p>{/if}
      </div>
    {/if}
  {:else if onboarding.name === 'load-error'}
    <p class="support">Onboarding could not start.</p>
    <div class="onboarding-actions">
      <button data-testid="onboarding-picker" class="primary" onclick={chooseHome}>Choose folder…</button>
      <button onclick={loadOnboarding}>Try again</button>
    </div>
    <p class="onboarding-error" role="alert">{onboarding.error}</p>
  {/if}
  </div>
  {#if ['choosing', 'confirming', 'settings', 'confirming-settings'].includes(onboarding.name)}
    <footer class="onboarding-footer">
      {#if onboarding.savedHomePath}<button data-testid="onboarding-cancel" onclick={() => { onboarding = onboardingCancelSettingsState(onboarding) }} disabled={onboarding.name === 'confirming-settings'}>Cancel</button>{:else}<span class="privacy-note">Plain Markdown · stored locally</span>{/if}
      <button data-testid="onboarding-confirm" class="primary" onclick={confirmHome} disabled={onboarding.name.startsWith('confirming') || !onboarding.homePath}>{onboarding.name.startsWith('confirming') ? 'Creating Home…' : 'Confirm and continue'}</button>
    </footer>
  {:else if ['import-choice', 'previewing', 'reviewing', 'extracting', 'finalizing'].includes(onboarding.name)}
    <footer class="onboarding-footer">
      {#if ['reviewing', 'extracting'].includes(onboarding.name)}<button data-testid="onboarding-import-picker" onclick={chooseImportArchive}>Choose a different ZIP…</button>{:else}<span class="privacy-note">Local preview · no Home writes</span>{/if}
      <div class="onboarding-actions"><button data-testid="onboarding-import-skip" onclick={skipImport} disabled={onboarding.name === 'finalizing'}>{onboarding.name === 'finalizing' ? 'Creating Home…' : 'Continue without importing'}</button>{#if ['reviewing', 'extracting'].includes(onboarding.name)}<button data-testid="onboarding-import-continue" class="primary" onclick={extractImportSelection} disabled={onboarding.name === 'extracting' || onboarding.selectedNames.length === 0}>{onboarding.name === 'extracting' ? 'Reading approved files…' : 'Continue with selected'}</button>{/if}</div>
    </footer>
  {:else if ['approved-review', 'import-saving', 'import-invalid'].includes(onboarding.name)}
    <footer class="onboarding-footer">
      {#if onboarding.name === 'import-invalid'}
        <button data-testid="onboarding-import-recover" onclick={returnToArchiveReview}>Back to archive review</button>
      {:else}
        <span class="privacy-note">Local import · reviewed files only</span>
        <button data-testid="onboarding-import-save" class="primary" onclick={saveConfirmedImport} disabled={onboarding.name === 'import-saving'}>{onboarding.name === 'import-saving' ? 'Saving Home…' : 'Save Home and finish'}</button>
      {/if}
    </footer>
  {/if}
</section>
{/if}

<style>
  .onboarding { width: min(680px, calc(100vw - 48px)); max-height: 100%; min-height: 0; display: flex; flex-direction: column; align-self: center; overflow: hidden; }
  .onboarding > header { flex: none; }
  .onboarding-content { min-height: 0; overflow-y: auto; }
  .onboarding h1 { margin: 4px 0 10px; font-size: var(--text-28); letter-spacing: -.02em; }
  .eyebrow, .path-label, .privacy-note { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .path-card { display: grid; grid-template-columns: 1fr auto; gap: 7px 16px; align-items: center; margin-top: 24px; padding: 15px 16px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); }
  .path-label { grid-column: 1 / -1; }
  .path-card strong { min-width: 0; overflow: hidden; text-overflow: ellipsis; font: var(--text-13) var(--font-mono); white-space: nowrap; }
  .folder-preview { display: flex; flex-wrap: wrap; gap: 8px; margin: 12px 0 0; }
  .folder-preview span { padding: 4px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .onboarding-error { margin: 10px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); line-height: 1.5; }
  .onboarding-footer { display: flex; flex: none; justify-content: space-between; align-items: center; gap: 16px; padding-top: 22px; }
  .primary { background: var(--ink); border-color: var(--ink); color: var(--paper); }
  .manifest-summary { display: flex; justify-content: space-between; gap: 16px; margin-top: 22px; padding-bottom: 9px; border-bottom: 1px solid var(--border); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .manifest-summary strong { color: var(--ink); font-weight: 500; }
  .manifest { margin: 0; padding: 0; list-style: none; }
  .manifest li { padding: 12px 0; border-bottom: 1px solid var(--border); }
  .manifest-consent { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 10px; align-items: start; cursor: pointer; }
  .manifest-consent input { margin-top: 2px; accent-color: var(--ink); }
  .manifest-meta { display: flex; justify-content: space-between; gap: 16px; font: var(--text-12) var(--font-mono); }
  .manifest-meta strong { min-width: 0; overflow-wrap: anywhere; font-weight: 500; }
  .manifest-meta span { flex: 0 0 auto; color: var(--muted); }
  .manifest pre { margin: 8px 0 0; padding: 8px 10px; border-radius: var(--radius-control); background: var(--faint); color: var(--muted); font: var(--text-12) var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  .empty-manifest { margin: 0; padding: 18px 0; border-bottom: 1px solid var(--border); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .onboarding-actions { display: flex; gap: 8px; }
  .approved-sources { margin: 20px 0 0; padding: 0; border-top: 1px solid var(--border); list-style: none; }
  .approved-sources li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 12px; padding: 8px 0; border-bottom: 1px solid var(--border); font: var(--text-12) var(--font-mono); }
  .approved-sources strong { overflow-wrap: anywhere; font-weight: 500; }
  .approved-sources span { color: var(--muted); overflow-wrap: anywhere; }
  .support { color: var(--muted); }
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { border-color: var(--muted); }
  button:disabled { color: var(--muted); cursor: default; }
</style>
