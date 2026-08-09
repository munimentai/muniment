import { describe, expect, it } from 'vitest'

import { onboardingCancelSettingsState, onboardingConfirmedHomePathState, onboardingConfirmedState, onboardingConfirmingState, onboardingErrorState, onboardingExtractingState, onboardingExtractionErrorState, onboardingExtractionState, onboardingFinalizingState, onboardingImportChoiceState, onboardingImportErrorState, onboardingImportSavedState, onboardingImportSavingState, onboardingLoadingState, onboardingPathState, onboardingPreviewErrorState, onboardingPreviewingState, onboardingPreviewState, onboardingReturnToArchiveReviewState, onboardingSelectionState, onboardingSettingsState, onboardingStatusState } from './onboarding-state.js'

describe('Home onboarding state', () => {
  it('blocks on the default location until it is configured', () => {
    expect(onboardingLoadingState).toEqual({ name: 'loading', homePath: '' })
    expect(onboardingStatusState({ configured: false, homePath: '/Documents/Muniment' })).toEqual({
      name: 'choosing',
      homePath: '/Documents/Muniment',
    })
  })

  it('opens settings and cancels back to the saved Home', () => {
    const settings = onboardingSettingsState({ name: 'complete', homePath: '/data/Muniment' })
    expect(settings).toEqual({ name: 'settings', homePath: '/data/Muniment', savedHomePath: '/data/Muniment' })
    const changed = onboardingPathState(settings, '/other/Muniment')
    expect(onboardingCancelSettingsState(changed)).toEqual({ name: 'complete', homePath: '/data/Muniment' })
  })

  it('completes when a persisted location is loaded', () => {
    expect(onboardingStatusState({ configured: true, homePath: '/data/Muniment' })).toEqual({
      name: 'complete',
      homePath: '/data/Muniment',
    })
  })

  it('keeps confirmation failures recoverable and clears stale errors on a new choice', () => {
    const confirming = onboardingConfirmingState({ name: 'choosing', homePath: '/read-only/Muniment' })
    const failed = onboardingErrorState(confirming, 'Muniment Home could not be created.')
    expect(failed).toMatchObject({ name: 'choosing', homePath: '/read-only/Muniment', error: 'Muniment Home could not be created.' })
    expect(onboardingPathState(failed, '/Documents/Muniment')).toEqual({
      name: 'choosing',
      homePath: '/Documents/Muniment',
      error: undefined,
    })
  })

  it('offers import preview before first-run Home confirmation', () => {
    const choice = onboardingImportChoiceState({ name: 'choosing', homePath: '/Documents/Muniment' })
    expect(choice).toEqual({ name: 'import-choice', homePath: '/Documents/Muniment', error: undefined })
    const finalizing = onboardingFinalizingState(choice)
    const confirmed = onboardingConfirmedState(
      finalizing,
      { configured: true, homePath: '/Documents/Muniment' },
    )
    expect(confirmed).toEqual({ name: 'complete', homePath: '/Documents/Muniment' })
    expect(onboardingConfirmedState(
      { name: 'confirming-settings', homePath: '/new/Home', savedHomePath: '/old/Home' },
      { configured: true, homePath: '/new/Home' },
    )).toEqual({ name: 'complete', homePath: '/new/Home' })
  })

  it('keeps Home and selected archive through preview success and typed failure', () => {
    const pending = onboardingPreviewingState({ name: 'import-choice', homePath: '/Documents/Muniment' }, '/Exports/data.zip')
    expect(pending).toMatchObject({ name: 'previewing', homePath: '/Documents/Muniment', archivePath: '/Exports/data.zip' })
    expect(onboardingPreviewState(pending, { entries: [], totalByteSize: 0 })).toMatchObject({
      name: 'reviewing',
      homePath: '/Documents/Muniment',
      archivePath: '/Exports/data.zip',
      manifest: { entries: [], totalByteSize: 0 },
    })
    expect(onboardingPreviewErrorState(pending, { kind: 'encrypted' })).toMatchObject({
      name: 'import-choice',
      homePath: '/Documents/Muniment',
      archivePath: '/Exports/data.zip',
      error: 'That archive contains an encrypted file. Choose an unencrypted export ZIP.',
    })
  })

  it('tracks explicit consent and retains it across retryable extraction failures', () => {
    const review = onboardingPreviewState(
      onboardingPreviewingState({ homePath: '/Home' }, '/export.zip'),
      { entries: [{ name: 'a.md' }, { name: 'b.json' }], totalByteSize: 2 },
    )
    expect(review.selectedNames).toEqual([])
    const selected = onboardingSelectionState(review, 'b.json', true)
    expect(selected.selectedNames).toEqual(['b.json'])
    expect(onboardingSelectionState(selected, 'b.json', false).selectedNames).toEqual([])
    const extracting = onboardingExtractingState(selected)
    expect(onboardingExtractionErrorState(extracting, { kind: 'unknownSelection' })).toMatchObject({
      name: 'reviewing', selectedNames: ['b.json'], manifest: review.manifest,
    })
    const extractedEntries = [{ sourceName: 'b.json', text: '{}', sourceProvenance: 'stable' }]
    expect(onboardingExtractionState(extracting, extractedEntries)).toMatchObject({ name: 'approved-review', extractedEntries })
    expect(onboardingFinalizingState(extracting)).toEqual({ name: 'finalizing', homePath: '/Home', error: undefined })
  })

  it('keeps approved sources through save review and return', () => {
    const extractedEntries = [{ sourceName: 'profile.json', text: '{}', sourceProvenance: 'assistant-export:stable' }]
    const review = { name: 'approved-review', homePath: '/Home', archivePath: '/export.zip', manifest: { entries: [{ name: 'profile.json' }] }, selectedNames: ['profile.json'], extractedEntries }
    expect(onboardingReturnToArchiveReviewState(review)).toMatchObject({ name: 'reviewing', selectedNames: ['profile.json'], manifest: review.manifest, extractedEntries })
  })

  it('retains confirmed import inputs through saving and typed recovery states', () => {
    const confirmed = {
      name: 'approved-review', homePath: '/Home',
      extractedEntries: [{ sourceName: 'profile.json', text: '{}' }],
      manifest: { entries: [{ name: 'profile.json' }] }, selectedNames: ['profile.json'],
    }
    const saving = onboardingImportSavingState(confirmed)
    expect(saving).toMatchObject({ name: 'import-saving', homePath: '/Home', extractedEntries: confirmed.extractedEntries })
    expect(onboardingImportSavedState(saving)).toEqual({ name: 'complete', homePath: '/Home' })

    const conflict = onboardingImportErrorState(saving, { kind: 'destinationConflict', relativePath: 'memory/profile.md', message: 'private detail' })
    expect(conflict).toMatchObject({ name: 'approved-review', errorKind: 'destinationConflict', conflictPath: 'memory/profile.md', extractedEntries: confirmed.extractedEntries })
    expect(conflict.error).not.toContain('private')
    expect(onboardingConfirmedHomePathState(conflict, '/Other')).toMatchObject({
      name: 'approved-review', homePath: '/Other', extractedEntries: confirmed.extractedEntries, error: undefined,
    })

    const saveFailed = onboardingImportErrorState(saving, { kind: 'saveFailed', message: 'disk secret' })
    expect(saveFailed).toMatchObject({ name: 'approved-review', errorKind: 'saveFailed', extractedEntries: confirmed.extractedEntries })
    expect(saveFailed.error).not.toContain('secret')
    const secret = onboardingImportErrorState(saving, { kind: 'secretRejected', sourceName: 'profile.json', message: 'ghp_leaked' })
    expect(secret).toMatchObject({ name: 'approved-review', errorKind: 'secretRejected', conflictPath: undefined, extractedEntries: confirmed.extractedEntries })
    expect(secret.error).toContain('credential')
    expect(secret.error).toContain('archive review')
    expect(secret.error).toContain('profile.json')
    expect(secret.error).not.toContain('ghp_')

    const invalid = onboardingImportErrorState(saving, { kind: 'invalidInput', message: 'parser secret' })
    expect(invalid).toMatchObject({ name: 'import-invalid', errorKind: 'invalidInput', extractedEntries: confirmed.extractedEntries })
    expect(invalid.error).not.toContain('secret')
  })
})
