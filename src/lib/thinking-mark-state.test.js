import { afterEach, describe, expect, it, vi } from 'vitest'
import { DEFAULT_MARK, MARK_EVENT, MARK_STORAGE_KEY, commitMark, parseMark, readStoredMark } from './thinking-mark-state.js'

afterEach(() => localStorage.clear())

describe('thinking mark preference', () => {
  it('defaults to the motion elephant and ignores unknown values', () => {
    expect(readStoredMark()).toBe(DEFAULT_MARK)
    expect(DEFAULT_MARK).toBe('motion')
    expect(parseMark('dancing')).toBe('motion')
    expect(parseMark('text')).toBe('text')
  })

  it('stores and announces a choice', () => {
    const heard = vi.fn()
    window.addEventListener(MARK_EVENT, heard)
    expect(commitMark('ear')).toBe('ear')
    window.removeEventListener(MARK_EVENT, heard)
    expect(localStorage.getItem(MARK_STORAGE_KEY)).toBe('ear')
    expect(readStoredMark()).toBe('ear')
    expect(heard.mock.calls[0][0].detail).toBe('ear')
  })
})
