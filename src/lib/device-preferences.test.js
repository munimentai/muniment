import { afterEach, expect, it } from 'vitest'

import { readDevicePreference, saveDevicePreference } from './device-preferences.js'

afterEach(() => localStorage.clear())

it('reads the fallback, merges a stored choice over it, and survives a malformed value', () => {
  expect(readDevicePreference('muniment.test', { enabled: true, keep: 7 })).toEqual({ enabled: true, keep: 7 })
  saveDevicePreference('muniment.test', { keep: 30 })
  expect(readDevicePreference('muniment.test', { enabled: true, keep: 7 })).toEqual({ enabled: true, keep: 30 })
  localStorage.setItem('muniment.test', '{')
  expect(readDevicePreference('muniment.test', { enabled: false })).toEqual({ enabled: false })
})
