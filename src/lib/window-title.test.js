import { describe, expect, it, vi } from 'vitest'

import { threadTitle } from './thread-title.js'
import { createWindowTitle, formatWindowTitle } from './window-title.js'

describe('window title', () => {
  it('uses the app name when no thread is open', () => {
    expect(formatWindowTitle()).toBe('muniment')
  })

  it('joins the bounded thread title to the app name', () => {
    const title = threadTitle([{ role: 'user', text: '😀'.repeat(81) }])

    expect(formatWindowTitle(title)).toBe(`${'😀'.repeat(79)}… | muniment`)
  })

  it('does nothing when the window API is missing', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})

    await expect(createWindowTitle().set('Lease renewal')).resolves.toBeUndefined()

    expect(error).not.toHaveBeenCalled()
  })

  it('ignores a rejected title change', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    const windowApi = { setTitle: vi.fn().mockRejectedValue(new Error('unavailable')) }

    await expect(createWindowTitle(windowApi).set('Lease renewal')).resolves.toBeUndefined()

    expect(windowApi.setTitle).toHaveBeenCalledWith('Lease renewal | muniment')
    expect(error).not.toHaveBeenCalled()
  })
})
