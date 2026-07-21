import { describe, expect, it } from 'vitest'

import { onboardingCancelSettingsState, onboardingCompleteState, onboardingConfirmedState, onboardingConfirmingState, onboardingErrorState, onboardingLoadingState, onboardingPathState, onboardingPreviewErrorState, onboardingPreviewingState, onboardingPreviewState, onboardingSettingsState, onboardingStatusState } from './onboarding-state.js'

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

  it('offers import preview only after first-run Home confirmation', () => {
    const confirmed = onboardingConfirmedState(
      { name: 'confirming', homePath: '/Documents/Muniment' },
      { configured: true, homePath: '/Documents/Muniment' },
    )
    expect(confirmed).toEqual({ name: 'import-choice', homePath: '/Documents/Muniment' })
    expect(onboardingCompleteState(confirmed)).toEqual({ name: 'complete', homePath: '/Documents/Muniment' })
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
})
