import { describe, expect, it } from 'vitest'

import {
  BASE_TEXT_TOKENS,
  SIZE_STEP_MAX,
  SIZE_STEP_MIN,
  applyType,
  bodySize,
  filterFonts,
  fontStack,
  parseType,
  serializeType,
  stepType,
  typeSizeShortcut,
  typeSizeShortcutLabel,
  typeSizeShortcutStep,
} from './type-state.js'

const root = () => {
  const style = new Map()
  return {
    style: {
      setProperty: (name, value) => style.set(name, value),
      removeProperty: (name) => style.delete(name),
      get: (name) => style.get(name),
      size: () => style.size,
    },
  }
}

describe('type state', () => {
  it('round-trips a state and restores the defaults for missing or malformed data', () => {
    expect(parseType(null)).toEqual({ step: 0, human: null, mono: null })
    expect(parseType('nonsense')).toEqual({ step: 0, human: null, mono: null })
    expect(parseType('{"step":9,"human":"","mono":42}')).toEqual({ step: 0, human: null, mono: null })
    expect(parseType('{"step":-1,"human":" Inter ","mono":"Iosevka"}')).toEqual({ step: -1, human: 'Inter', mono: 'Iosevka' })
    expect(parseType('{"step":1,"human":"Bad\\"; url(x)"}').human).toBeNull()
    expect(serializeType({ step: 2, human: 'Inter', mono: null })).toBe('{"step":2,"human":"Inter","mono":null}')
  })

  it('steps within the range and names the body size', () => {
    expect(stepType({ step: 0 }, 1).step).toBe(1)
    expect(stepType({ step: SIZE_STEP_MAX }, 1).step).toBe(SIZE_STEP_MAX)
    expect(stepType({ step: SIZE_STEP_MIN }, -1).step).toBe(SIZE_STEP_MIN)
    expect(bodySize(0)).toBe(15)
    expect(bodySize(1)).toBe(16.5)
    expect(bodySize(-2)).toBe(12)
  })

  it('scales every text token together and clears them at the default', () => {
    const target = root()
    applyType(target, { step: 2, human: null, mono: null })
    expect(target.style.get('--text-15')).toBe('18px')
    expect(target.style.get('--text-12')).toBe('14.4px')
    expect(target.style.get('--text-provenance')).toBe('13.8px')
    expect(target.style.size()).toBe(Object.keys(BASE_TEXT_TOKENS).length)
    applyType(target, { step: 0, human: null, mono: null })
    expect(target.style.size()).toBe(0)
  })

  it('puts a picked family in front of the shipped stack and clears it for the default', () => {
    expect(fontStack('human', 'Inter')).toBe("'Inter', 'Schibsted Grotesk', system-ui, sans-serif")
    expect(fontStack('mono', null)).toBeNull()
    const target = root()
    applyType(target, { step: 0, human: 'Inter', mono: 'Iosevka' })
    expect(target.style.get('--font-human')).toBe("'Inter', 'Schibsted Grotesk', system-ui, sans-serif")
    expect(target.style.get('--font-mono')).toBe("'Iosevka', 'Commit Mono', ui-monospace, monospace")
    applyType(target, { step: 0, human: null, mono: null })
    expect(target.style.size()).toBe(0)
  })

  it('reads the super key with equals, minus and zero on each platform', () => {
    const mac = 'MacIntel'
    const linux = 'Linux x86_64'
    expect(typeSizeShortcutStep({ key: '=', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }, mac)).toBe(1)
    expect(typeSizeShortcutStep({ key: '+', metaKey: true, ctrlKey: false, altKey: false, shiftKey: true }, mac)).toBe(1)
    expect(typeSizeShortcutStep({ key: '-', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }, mac)).toBe(-1)
    expect(typeSizeShortcutStep({ key: '0', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }, mac)).toBe(0)
    expect(typeSizeShortcutStep({ key: '=', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false }, mac)).toBeNull()
    expect(typeSizeShortcutStep({ key: '=', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false }, linux)).toBe(1)
    expect(typeSizeShortcutStep({ key: '=', metaKey: true, ctrlKey: true, altKey: false, shiftKey: false }, linux)).toBeNull()
    expect(typeSizeShortcutStep({ key: '=', metaKey: true, ctrlKey: false, altKey: true, shiftKey: false }, mac)).toBeNull()
    expect(typeSizeShortcut('larger', mac)).toBe('Meta+=')
    expect(typeSizeShortcut('smaller', linux)).toBe('Control+-')
    expect(typeSizeShortcut('default', mac)).toBe('Meta+0')
    expect(typeSizeShortcutLabel('default', mac)).toBe('⌘ 0')
  })

  it('filters installed families by name, once each, up to the cap', () => {
    const fonts = ['Avenir Next', 'Inter', 'inter', 'Iosevka', '', 'IBM Plex Mono']
    expect(filterFonts(fonts, '')).toEqual(['Avenir Next', 'Inter', 'Iosevka', 'IBM Plex Mono'])
    expect(filterFonts(fonts, ' io')).toEqual(['Iosevka'])
    expect(filterFonts(fonts, 'i', 2)).toEqual(['Avenir Next', 'Inter'])
    expect(filterFonts(null, 'x')).toEqual([])
  })
})
