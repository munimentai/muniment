import { describe, expect, it } from 'vitest'

import {
  PAIRING_POLL_MS,
  PAIRING_REPLACE_MS,
  challengeExpired,
  pairingChallengeState,
  remainingLabel,
  replacementReady,
} from './pairing-state.js'

describe('pairing bounds', () => {
  it('polls at most once every two seconds', () => {
    expect(PAIRING_POLL_MS).toBe(2000)
  })

  it('waits ten seconds before replacing a challenge', () => {
    expect(PAIRING_REPLACE_MS).toBe(10000)
    expect(replacementReady(0, 9999)).toBe(false)
    expect(replacementReady(0, 10000)).toBe(true)
  })

  it('expires the display at the server deadline', () => {
    const expiresAt = '2026-01-01T00:02:00.000Z'
    const deadline = Date.parse(expiresAt)
    expect(challengeExpired(expiresAt, deadline - 1)).toBe(false)
    expect(challengeExpired(expiresAt, deadline)).toBe(true)
    expect(remainingLabel(expiresAt, deadline - 19000)).toBe('00:19')
    expect(remainingLabel(expiresAt, deadline)).toBe('00:00')
  })

  it('keeps challenge material off the display state name', () => {
    const state = pairingChallengeState({
      expires_at: '2026-01-01T00:02:00.000Z',
      qr_svg: '<svg xmlns="http://www.w3.org/2000/svg"></svg>',
    }, 1)
    expect(state.name).toBe('challenge')
    expect(JSON.stringify(state)).not.toContain('challenge":')
    expect(JSON.stringify(state)).not.toContain('access_token')
    expect(JSON.stringify(state)).not.toContain('https://')
  })
})
