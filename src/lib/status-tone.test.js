import { describe, expect, it } from 'vitest'
import { machineStatusTone, onlineTone, runStatusTone, updateStatusTone } from './status-tone.js'

describe('status tones', () => {
  it('colors run states', () => {
    expect(['queued', 'leased', 'running', 'waiting_input', 'completed', 'failed', 'cancelled', 'interrupted', 'new'].map(runStatusTone))
      .toEqual(['warning', 'active', 'active', 'warning', 'success', 'danger', 'neutral', 'warning', 'neutral'])
  })

  it('colors machine and swarm states', () => {
    expect(['online', 'connecting', 'offline', 'unknown', 'version_mismatch', 'revoked', undefined].map(machineStatusTone))
      .toEqual(['success', 'active', 'neutral', 'neutral', 'warning', 'danger', 'neutral'])
    expect([true, false].map(onlineTone)).toEqual(['success', 'neutral'])
  })

  it('colors update states', () => {
    expect(['queued', 'requested', 'available', 'downloading', 'installing', 'failed', null].map(updateStatusTone))
      .toEqual(['warning', 'warning', 'warning', 'active', 'active', 'danger', 'neutral'])
  })
})
