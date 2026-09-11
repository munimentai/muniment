import fs from 'node:fs'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { firstRunError, firstRunErrors } from './onboarding-diagnostics.js'

afterEach(() => vi.restoreAllMocks())

describe('The shell reports first-run errors.', () => {
  it.each(Object.entries(firstRunErrors))('The shell reports %s without user data.', async (cause, message) => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    const tauri = { invoke: vi.fn().mockResolvedValue(undefined) }
    const error = firstRunError(tauri, cause)
    expect(error.message).toBe(message)
    expect(log).toHaveBeenCalledWith(message)
    expect(tauri.invoke).toHaveBeenCalledWith('onboarding_model_settings_error', { cause })
    const backend = fs.readFileSync('src-tauri/src/onboarding_diagnostics.rs', 'utf8')
    expect(backend).toContain(JSON.stringify(message))
  })

  it('The shell keeps the visible cause when stderr fails.', async () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {})
    const tauri = { invoke: vi.fn().mockRejectedValue(new Error('transport failed')) }
    expect(firstRunError(tauri, 'runtime').message).toBe(firstRunErrors.runtime)
    await vi.waitFor(() => expect(log).toHaveBeenCalledWith('Muniment could not write the first-run error to stderr.'))
  })
})
