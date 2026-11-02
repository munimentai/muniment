import { describe, expect, it } from 'vitest'

import { readRunSteers, rememberRunSteer, RUN_STEERS_KEY } from './run-steers.js'

function memoryStorage() {
  const values = new Map()
  return { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, String(value)) }
}

describe('run steers', () => {
  it('keeps each reply\'s messages in order and drops malformed entries', () => {
    const storage = memoryStorage()
    rememberRunSteer('run-1', { text: 'foxtrot', textOffset: 4 }, storage)
    rememberRunSteer('run-1', { text: 'golf', textOffset: 11 }, storage)
    expect(readRunSteers(storage)).toEqual({ 'run-1': [{ text: 'foxtrot', textOffset: 4 }, { text: 'golf', textOffset: 11 }] })
    storage.setItem(RUN_STEERS_KEY, JSON.stringify({ 'run-2': [{ text: 'x', textOffset: -1 }], 'run-3': 'bad' }))
    expect(readRunSteers(storage)).toEqual({})
    storage.setItem(RUN_STEERS_KEY, 'not json')
    expect(readRunSteers(storage)).toEqual({})
  })

  it('keeps the newest 200 replies', () => {
    const storage = memoryStorage()
    for (let index = 0; index < 205; index += 1) rememberRunSteer(`run-${index}`, { text: 'm', textOffset: 0 }, storage)
    const kept = Object.keys(readRunSteers(storage))
    expect(kept).toHaveLength(200)
    expect(kept[0]).toBe('run-5')
  })
})
