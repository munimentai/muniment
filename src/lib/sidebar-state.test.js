import { describe, expect, it, vi } from 'vitest'

import {
  readSidebarCollapsed,
  sidebarStorageKey,
  writeSidebarCollapsed,
} from './sidebar-state.js'

describe('sidebar collapse persistence', () => {
  it('defaults to expanded and restores only an explicitly collapsed sidebar', () => {
    expect(readSidebarCollapsed({ getItem: () => null })).toBe(false)
    expect(readSidebarCollapsed({ getItem: () => 'false' })).toBe(false)
    expect(readSidebarCollapsed({ getItem: () => 'true' })).toBe(true)
  })

  it('stores the current state', () => {
    const setItem = vi.fn()
    writeSidebarCollapsed({ setItem }, true)
    expect(setItem).toHaveBeenCalledWith(sidebarStorageKey, 'true')
  })

  it('falls back safely when storage is unavailable', () => {
    const unavailable = {
      getItem() { throw new Error('blocked') },
      setItem() { throw new Error('blocked') },
    }
    expect(readSidebarCollapsed(unavailable)).toBe(false)
    expect(() => writeSidebarCollapsed(unavailable, true)).not.toThrow()
  })
})
