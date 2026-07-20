import { describe, expect, it } from 'vitest'

import { formatModelBytes, modelAcquisitionState } from './model-acquisition-state.js'

describe('model acquisition state', () => {
  it('clamps determinate progress and formats binary sizes', () => {
    expect(modelAcquisitionState({
      status: { state: 'installing' }, downloadedBytes: 3 * 1024 ** 3, totalBytes: 4 * 1024 ** 3,
    })).toMatchObject({ active: true, ready: false, percent: 75 })
    expect(modelAcquisitionState({
      status: { state: 'installing' }, downloadedBytes: 8, totalBytes: 4,
    }).percent).toBe(100)
    expect(formatModelBytes(3.5 * 1024 ** 3)).toBe('3.5 GB')
  })

  it('treats background retry as active and readiness as terminal', () => {
    expect(modelAcquisitionState({ status: { state: 'failed' }, retryingInBackground: true })).toMatchObject({ active: true, failed: true, retrying: true })
    expect(modelAcquisitionState({ status: { state: 'installed' }, aiFeaturesAvailable: true, retryingInBackground: true })).toMatchObject({ active: false, ready: true })
  })

  it('handles missing and invalid byte totals without invalid progress', () => {
    expect(modelAcquisitionState({ status: { state: 'failed' }, downloadedBytes: -1, totalBytes: 'unknown' })).toMatchObject({ active: false, percent: 0, downloadedBytes: 0, totalBytes: 0 })
    expect(formatModelBytes(Number.NaN)).toBe('0 B')
  })
})
