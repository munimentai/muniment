// @vitest-environment jsdom

import { describe, expect, it } from 'vitest'

import {
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  SIDEBAR_STORAGE_KEY,
  SIDEBAR_WIDTH_STORAGE_KEY,
  clampSidebarWidth,
  createSidebarResizeController,
  sidebarWidthFromKey,
  sidebarWidthFromPointer,
  storedSidebarWidth,
  isSidebarShortcut,
  isNewThreadShortcut,
  newThreadShortcut,
  parseSidebarCollapsed,
  serializeSidebarCollapsed,
  sidebarShortcut,
  threadRowShortcut,
  threadRowShortcutPosition,
} from './sidebar-state.js'

describe('sidebar state', () => {
  it('uses the platform primary modifier', () => {
    expect(sidebarShortcut('MacIntel')).toBe('Meta+\\')
    expect(sidebarShortcut('Win32')).toBe('Control+\\')
    expect(sidebarShortcut('Linux x86_64')).toBe('Control+\\')
  })

  it('recognizes only the platform shortcut without extra modifiers', () => {
    expect(isSidebarShortcut(new KeyboardEvent('keydown', { key: '\\', metaKey: true }), 'MacIntel')).toBe(true)
    expect(isSidebarShortcut(new KeyboardEvent('keydown', { key: '\\', ctrlKey: true }), 'Linux x86_64')).toBe(true)
    expect(isSidebarShortcut(new KeyboardEvent('keydown', { key: '\\', ctrlKey: true }), 'MacIntel')).toBe(false)
    expect(isSidebarShortcut(new KeyboardEvent('keydown', { key: '\\', metaKey: true }), 'Win32')).toBe(false)
    expect(isSidebarShortcut(new KeyboardEvent('keydown', { key: '\\', ctrlKey: true, shiftKey: true }), 'Win32')).toBe(false)
    expect(isSidebarShortcut(new KeyboardEvent('keydown', { key: '\\', ctrlKey: true, altKey: true }), 'Win32')).toBe(false)
    expect(isSidebarShortcut(new KeyboardEvent('keydown', { key: '|', ctrlKey: true }), 'Win32')).toBe(false)
  })

  it('recognizes the shortcut from editable controls', () => {
    for (const target of [document.createElement('input'), document.createElement('textarea')]) {
      expect(isSidebarShortcut({ key: '\\', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, target }, 'Win32')).toBe(true)
    }
    const editable = document.createElement('div')
    editable.setAttribute('contenteditable', 'true')
    expect(isSidebarShortcut({ key: '\\', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, target: editable }, 'Win32')).toBe(true)
  })

  it('round-trips the remembered choice and defaults missing or malformed data to expanded', () => {
    expect(SIDEBAR_STORAGE_KEY).toBe('muniment.sidebar-collapsed')
    expect(parseSidebarCollapsed(serializeSidebarCollapsed(true))).toBe(true)
    expect(parseSidebarCollapsed(serializeSidebarCollapsed(false))).toBe(false)
    expect(parseSidebarCollapsed(null)).toBe(false)
    expect(parseSidebarCollapsed(undefined)).toBe(false)
    expect(parseSidebarCollapsed('')).toBe(false)
    expect(parseSidebarCollapsed('true')).toBe(false)
    expect(parseSidebarCollapsed('{"collapsed":true}')).toBe(false)
  })
})

describe('new thread shortcut', () => {
  it('uses the platform modifier', () => {
    expect(newThreadShortcut('MacIntel')).toBe('Meta+N')
    expect(newThreadShortcut('Win32')).toBe('Control+N')
  })

  it('matches only the platform chord from editable targets', () => {
    const target = document.createElement('textarea')
    expect(isNewThreadShortcut({ key: 'n', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false, target }, 'MacIntel')).toBe(true)
    expect(isNewThreadShortcut({ key: 'N', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false, target }, 'Win32')).toBe(true)
    expect(isNewThreadShortcut({ key: 'n', metaKey: false, ctrlKey: true, altKey: false, shiftKey: true, target }, 'Win32')).toBe(false)
  })
})

describe('thread row shortcuts', () => {
  it('uses the platform modifier in accessibility labels', () => {
    expect(threadRowShortcut(1, 'MacIntel')).toBe('Meta+1')
    expect(threadRowShortcut(9, 'Win32')).toBe('Control+9')
  })

  it('maps physical digit chords to positions', () => {
    expect(threadRowShortcutPosition({ code: 'Digit1', key: '&', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }, 'MacIntel')).toBe(1)
    expect(threadRowShortcutPosition({ code: 'Digit9', key: '9', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false }, 'Linux x86_64')).toBe(9)
  })

  it('rejects other keys, modifiers, and numpad digits', () => {
    const chord = { code: 'Digit4', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false }
    expect(threadRowShortcutPosition({ ...chord, code: 'Digit0' }, 'Win32')).toBeNull()
    expect(threadRowShortcutPosition({ ...chord, code: 'Numpad4' }, 'Win32')).toBeNull()
    expect(threadRowShortcutPosition({ ...chord, metaKey: true }, 'Win32')).toBeNull()
    expect(threadRowShortcutPosition({ ...chord, ctrlKey: false, metaKey: true }, 'Win32')).toBeNull()
    expect(threadRowShortcutPosition({ ...chord, altKey: true }, 'Win32')).toBeNull()
    expect(threadRowShortcutPosition({ ...chord, shiftKey: true }, 'Win32')).toBeNull()
    expect(threadRowShortcutPosition({ ...chord, ctrlKey: true, metaKey: false }, 'MacIntel')).toBeNull()
  })
})

describe('sidebar width', () => {
  it('is three quarters of the old 260px column by default and clamps to its range', () => {
    expect(SIDEBAR_DEFAULT_WIDTH).toBe(195)
    expect(clampSidebarWidth(100)).toBe(SIDEBAR_MIN_WIDTH)
    expect(clampSidebarWidth(900)).toBe(SIDEBAR_MAX_WIDTH)
    expect(clampSidebarWidth(300.4)).toBe(300)
    expect(clampSidebarWidth(Number.NaN)).toBe(SIDEBAR_DEFAULT_WIDTH)
    expect(clampSidebarWidth(300, 240)).toBe(240)
    expect(clampSidebarWidth(300, 40)).toBe(SIDEBAR_MIN_WIDTH)
  })

  it('measures the width from the workspace left edge to the pointer', () => {
    expect(sidebarWidthFromPointer(308, 8)).toBe(300)
    expect(sidebarWidthFromPointer(20, 8)).toBe(SIDEBAR_MIN_WIDTH)
    expect(sidebarWidthFromPointer(900, 8)).toBe(SIDEBAR_MAX_WIDTH)
    expect(sidebarWidthFromPointer(400, 8, 250)).toBe(250)
  })

  it('steps with the arrow keys toward the thread and back', () => {
    expect(sidebarWidthFromKey(200, 'ArrowRight')).toBe(220)
    expect(sidebarWidthFromKey(200, 'ArrowLeft')).toBe(180)
    expect(sidebarWidthFromKey(200, 'Home')).toBe(SIDEBAR_MIN_WIDTH)
    expect(sidebarWidthFromKey(200, 'End')).toBe(SIDEBAR_MAX_WIDTH)
    expect(sidebarWidthFromKey(200, 'End', 260)).toBe(260)
    expect(sidebarWidthFromKey(200, 'Enter')).toBe(200)
  })

  it('reads the kept width and falls back to the default', () => {
    const storage = new Map()
    const store = { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) }
    expect(storedSidebarWidth(store)).toBe(SIDEBAR_DEFAULT_WIDTH)
    store.setItem(SIDEBAR_WIDTH_STORAGE_KEY, '240')
    expect(storedSidebarWidth(store)).toBe(240)
    store.setItem(SIDEBAR_WIDTH_STORAGE_KEY, 'wide')
    expect(storedSidebarWidth(store)).toBe(SIDEBAR_DEFAULT_WIDTH)
    expect(storedSidebarWidth({ getItem() { throw new Error('blocked') } })).toBe(SIDEBAR_DEFAULT_WIDTH)
  })

  it('keeps every pointer and keyboard width per device', () => {
    const storage = new Map()
    const store = { getItem: (key) => storage.get(key) ?? null, setItem: (key, value) => storage.set(key, value) }
    let width = SIDEBAR_DEFAULT_WIDTH
    let pointer
    const controller = createSidebarResizeController({
      readWidth: () => width,
      readMaximum: () => 300,
      readPointer: () => pointer,
      readAvailableWidth: () => 300,
      readLeftEdge: () => 8,
      onWidth: (next) => { width = next },
      onMaximum: () => {},
      onPointer: (next) => { pointer = next },
      storage: store,
    })
    const target = { focus() { throw new Error('a pointer press never focuses the divider') }, setPointerCapture() {}, hasPointerCapture: () => false }
    controller.pointerDown({ button: 0, pointerId: 3, currentTarget: target, preventDefault() {} })
    controller.pointerMove({ pointerId: 3, clientX: 258 })
    expect(width).toBe(250)
    expect(store.getItem(SIDEBAR_WIDTH_STORAGE_KEY)).toBe('250')
    controller.pointerEnd({ pointerId: 3, currentTarget: target })
    controller.pointerMove({ pointerId: 3, clientX: 100 })
    expect(width).toBe(250)
    controller.keydown({ key: 'ArrowLeft', preventDefault() {} })
    expect(width).toBe(230)
    expect(store.getItem(SIDEBAR_WIDTH_STORAGE_KEY)).toBe('230')
    controller.fit()
    expect(width).toBe(230)
  })
})
