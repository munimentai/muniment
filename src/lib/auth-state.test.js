import { describe, expect, it } from 'vitest'

import { bootState, errorKind, errorState, statusState, waitingState } from './auth-state.js'

describe('auth state transitions', () => {
  it('starts in boot and resolves status to signed out', () => {
    expect(bootState).toEqual({ name: 'boot' })
    expect(statusState({ signed_in: false, subject: null })).toEqual({ name: 'signed-out' })
  })

  it('moves through waiting to a signed-in profile', () => {
    expect(waitingState()).toEqual({ name: 'signing-in' })
    expect(statusState({ signed_in: true, subject: 'mikey@example.com' })).toEqual({
      name: 'signed-in',
      subject: 'mikey@example.com',
    })
  })

  it('keeps the failed action available for retry', () => {
    expect(errorState('sign-in', 'the browser session was cancelled')).toEqual({
      name: 'error',
      message: 'Sign-in not completed — the browser session was cancelled. Try again.',
      retry: 'sign-in',
    })
  })

  it('classifies structured command errors without matching their message', () => {
    const error = { kind: 'network', message: 'network error: connection refused' }
    expect(errorKind(error)).toBe('network')
    expect(errorState('status', error).message).toBe(
      'Session status unavailable — network error: connection refused. Try again.',
    )
  })
})
