import { afterEach, describe, expect, it } from 'vitest'
import { addressFor, readSearchEngine, saveSearchEngine, DEFAULT_SEARCH } from './browser-settings.js'

afterEach(() => localStorage.clear())

describe('address bar', () => {
  it('opens addresses and searches words with the default engine', () => {
    expect(addressFor('example.com/a?b=1', 'google')).toBe('https://example.com/a?b=1')
    expect(addressFor('http://example.com', 'google')).toBe('http://example.com')
    expect(addressFor('localhost:5173', 'google')).toBe('http://localhost:5173')
    expect(addressFor('10.1.10.102:57090', 'google')).toBe('http://10.1.10.102:57090')
    expect(addressFor('3m.com', 'google')).toBe('https://3m.com')
    expect(addressFor('grace hopper', 'google')).toBe('https://www.google.com/search?q=grace%20hopper')
    expect(addressFor('3.14 pie', 'bing')).toBe('https://www.bing.com/search?q=3.14%20pie')
    expect(addressFor('   ', 'google')).toBeNull()
  })

  it('reads a bang before or after the words, and sends unknown bangs to DuckDuckGo', () => {
    expect(addressFor('!w ada lovelace', 'google')).toBe('https://en.wikipedia.org/wiki/Special:Search?search=ada%20lovelace')
    expect(addressFor('ada lovelace !YT', 'google')).toBe('https://www.youtube.com/results?search_query=ada%20lovelace')
    expect(addressFor('!gh', 'google')).toBe('https://github.com/')
    expect(addressFor('!zz thing', 'brave')).toBe('https://duckduckgo.com/?q=!zz%20thing')
  })

  it('remembers a listed engine and falls back to Google', () => {
    expect(readSearchEngine()).toBe(DEFAULT_SEARCH)
    saveSearchEngine('brave')
    expect(readSearchEngine()).toBe('brave')
    expect(() => saveSearchEngine('altavista')).toThrow()
    localStorage.setItem('muniment.browser.search', 'altavista')
    expect(readSearchEngine()).toBe('google')
  })
})
