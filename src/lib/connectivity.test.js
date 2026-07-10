import { describe, expect, it } from 'vitest'

import {
  connectedState,
  diagnosticsText,
  enterUnreachable,
  resetConnectivity,
  secondsUntilRetry,
} from './connectivity.js'

const failure = { action: 'status', message: 'network error: connection refused' }

describe('connectivity state', () => {
  it('enters unreachable and counts down without timers', () => {
    const state = enterUnreachable(connectedState, failure, 1_000)
    expect(state.name).toBe('unreachable')
    expect(secondsUntilRetry(state, 1_000)).toBe(5)
    expect(secondsUntilRetry(state, 5_001)).toBe(1)
    expect(secondsUntilRetry(state, 6_000)).toBe(0)
  })

  it('backs off repeated failures and caps at 60 seconds', () => {
    let state = connectedState
    const delays = []
    for (let attempt = 0; attempt < 6; attempt += 1) {
      state = enterUnreachable(state, failure, 0)
      delays.push(secondsUntilRetry(state, 0))
    }
    expect(delays).toEqual([5, 10, 30, 60, 60, 60])
  })

  it('resets the backoff after success', () => {
    const failedTwice = enterUnreachable(enterUnreachable(connectedState, failure, 0), failure, 0)
    expect(resetConnectivity(failedTwice)).toEqual(connectedState)
    expect(secondsUntilRetry(enterUnreachable(resetConnectivity(), failure, 0), 0)).toBe(5)
  })

  it('builds token-free diagnostics from explicit safe fields', () => {
    expect(
      diagnosticsText({
        version: '0.0.1',
        timestamp: 0,
        action: 'status',
        issuerHost: 'api.muniment.ai',
        message: failure.message,
      }),
    ).toBe(
      'Muniment Desktop 0.0.1\nTimestamp: 1970-01-01T00:00:00.000Z\nFailed action: status\nIssuer host: api.muniment.ai\nError: network error: connection refused',
    )
  })
})
