import { describe, expect, it } from 'vitest'

import { requiredModelMessage, requiredModelPollActive, requiredModelProgress } from './model-acquisition-state.js'

describe('required model acquisition state', () => {
  it('polls only during installation or background retry', () => {
    expect(requiredModelPollActive({ status: { state: 'installing' }, retryingInBackground: false })).toBe(true)
    expect(requiredModelPollActive({ status: { state: 'failed' }, retryingInBackground: true })).toBe(true)
    expect(requiredModelPollActive({ status: { state: 'installed' }, retryingInBackground: false })).toBe(false)
    expect(requiredModelPollActive({ status: { state: 'failed' }, retryingInBackground: false })).toBe(false)
  })

  it('bounds determinate progress to valid values', () => {
    expect(requiredModelProgress({ downloadedBytes: 120, totalBytes: 100 })).toEqual({ downloaded: 100, total: 100 })
    expect(requiredModelProgress({ downloadedBytes: -1, totalBytes: 0 })).toEqual({ downloaded: 0, total: 0 })
    expect(requiredModelProgress({ downloadedBytes: Number.NaN, totalBytes: Infinity })).toEqual({ downloaded: 0, total: 0 })
  })

  it('keeps failed and retrying states fail-open', () => {
    expect(requiredModelMessage({ status: { state: 'failed' }, aiFeaturesAvailable: false, retryingInBackground: false })).toContain('rest of the app remain usable')
    expect(requiredModelMessage({ status: { state: 'failed' }, aiFeaturesAvailable: false, retryingInBackground: true })).toContain('download retries')
  })
})
