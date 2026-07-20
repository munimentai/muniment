import { describe, expect, it } from 'vitest'

import { onboardingConfirmingState, onboardingErrorState, onboardingLoadingState, onboardingPathState, onboardingStatusState } from './onboarding-state.js'

describe('Home onboarding state', () => {
  it('blocks on the default location until it is configured', () => {
    expect(onboardingLoadingState).toEqual({ name: 'loading', homePath: '' })
    expect(onboardingStatusState({ configured: false, homePath: '/Documents/Muniment' })).toEqual({
      name: 'choosing',
      homePath: '/Documents/Muniment',
    })
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
})
