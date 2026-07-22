import { describe, expect, it } from 'vitest'

import { onboardingCancelSettingsState, onboardingConfirmedState, onboardingConfirmingState, onboardingErrorState, onboardingExtractingState, onboardingExtractionErrorState, onboardingExtractionState, onboardingFinalizingState, onboardingImportChoiceState, onboardingLoadingState, onboardingPathState, onboardingPreviewErrorState, onboardingPreviewingState, onboardingPreviewState, onboardingReturnToArchiveReviewState, onboardingSelectionState, onboardingSettingsState, onboardingStatusState, onboardingTriageConfirmedState, onboardingTriageErrorState, onboardingTriageReportState, onboardingTriagingState } from './onboarding-state.js'

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
    expect(onboardingExtractionState(extracting, extractedEntries)).toMatchObject({ name: 'pre-triage', extractedEntries })
    expect(onboardingFinalizingState(extracting)).toEqual({ name: 'finalizing', homePath: '/Home', error: undefined })
  })

  it('keeps approved sources through triage review, retry, confirmation, and return', () => {
    const extractedEntries = [{ sourceName: 'profile.json', text: '{}', sourceProvenance: 'assistant-export:stable' }]
    const ready = { name: 'pre-triage', homePath: '/Home', archivePath: '/export.zip', manifest: { entries: [{ name: 'profile.json' }] }, selectedNames: ['profile.json'], extractedEntries }
    const pending = onboardingTriagingState(ready)
    expect(pending).toMatchObject({ name: 'triaging', extractedEntries })
    const failed = onboardingTriageErrorState(pending, { kind: 'localAiUnavailable', message: 'sensitive detail' })
    expect(failed).toMatchObject({ name: 'triage-error', extractedEntries, error: 'Local AI is unavailable. Start the local model and try again.' })
    expect(failed.error).not.toContain('sensitive')
    const report = { userType: 'Writer', proposedHomeLayout: 'Projects by topic', starterAgents: ['Researcher', 'Editor'] }
    const review = onboardingTriageReportState(onboardingTriagingState(failed), { report, usage: null })
    expect(review).toMatchObject({ name: 'triage-review', report, extractedEntries })
    expect(onboardingTriageConfirmedState(review)).toMatchObject({ name: 'triage-confirmed', report, extractedEntries })
    expect(onboardingReturnToArchiveReviewState(review)).toMatchObject({ name: 'reviewing', selectedNames: ['profile.json'], manifest: ready.manifest, extractedEntries })
  })
})
