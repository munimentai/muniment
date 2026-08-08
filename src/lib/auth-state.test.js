import { describe, expect, it } from 'vitest'

import { accessErrorState, accessLoadingState, accessReadyState, bootState, companionsErrorState, companionsLoadingState, companionsReadyState, devicesErrorState, devicesLoadingState, devicesReadyState, errorState, platformDisplayName, registrationRetryState, statusState, waitingState } from './auth-state.js'

describe('auth state transitions', () => {
  it('starts in boot and resolves status to signed out', () => {
    expect(bootState).toEqual({ name: 'boot' })
    expect(statusState({ signed_in: false, subject: null })).toEqual({ name: 'signed-out' })
  })

  it('moves through waiting to a signed-in profile', () => {
    expect(waitingState()).toEqual({ name: 'signing-in', message: 'Waiting for the browser sign-in…' })
    expect(statusState({ signed_in: true, subject: 'mikey@example.com' })).toEqual({
      name: 'signed-in',
      subject: 'mikey@example.com',
    })
  })

  it('bounds the registration retry status delay', () => {
    expect(registrationRetryState(7).message).toBe('Server busy. Retrying in 7 s')
    expect(registrationRetryState('invalid').message).toBe('Server busy. Retrying in 30 s')
    expect(registrationRetryState(999).message).toBe('Server busy. Retrying in 300 s')
  })

  it('keeps the failed action available for retry', () => {
    expect(errorState('sign-in', 'the browser session was cancelled')).toEqual({
      name: 'error',
      message: 'Sign-in not completed: the browser session was cancelled.',
      retry: 'sign-in',
    })
  })
})

describe('device list state', () => {
  it('maps platform ids to display names', () => {
    expect(platformDisplayName('macos')).toBe('macOS')
    expect(platformDisplayName('ios')).toBe('iOS')
    expect(platformDisplayName('freebsd')).toBe('Freebsd')
    expect(platformDisplayName('constructor')).toBe('Constructor')
  })

  it('orders active before revoked and each state by latest activity with a stable tie break', () => {
    const devices = [
      { device_id: 'd', revoked_at: '2026-01-03T00:00:00Z', last_active_at: '2026-01-04T00:00:00Z' },
      { device_id: 'b', revoked_at: null, last_active_at: '2026-01-01T00:00:00Z' },
      { device_id: 'c', revoked_at: null, last_active_at: '2026-01-02T00:00:00Z' },
      { device_id: 'a', revoked_at: null, last_active_at: '2026-01-01T00:00:00Z' },
    ]
    expect(devicesReadyState(devices).devices.map(({ device_id }) => device_id)).toEqual(['c', 'a', 'b', 'd'])
    expect(devices).toHaveLength(4)
  })

  it('keeps loading and redacted failure independent', () => {
    expect(devicesLoadingState()).toEqual({ name: 'loading' })
    expect(devicesErrorState('backend secret')).toEqual({ name: 'error' })
  })
})

describe('access snapshot state', () => {
  it('projects empty grants as stable empty categories', () => {
    const state = accessReadyState({ groups: [{ name: 'everyone' }] })
    expect(state.groups[0]).toMatchObject({ models: [], connections: [], capabilities: [] })
  })

  it('keeps loading and retryable failure local to access state', () => {
    expect(accessLoadingState()).toEqual({ name: 'loading' })
    expect(accessErrorState('offline')).toEqual({ name: 'error', message: 'offline' })
  })
})

describe('connected program list state', () => {
  it('keeps loading, ready, and error states separate', () => {
    const records = [{ identity: 'client-1', claimed_kind: 'cli', claimed_version: '1.2.3', approved_at: null }]
    expect(companionsLoadingState()).toEqual({ name: 'loading' })
    expect(companionsReadyState(records)).toEqual({ name: 'ready', companions: records })
    expect(companionsErrorState('backend secret')).toEqual({ name: 'error' })
  })
})
