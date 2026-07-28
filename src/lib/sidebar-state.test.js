// @vitest-environment jsdom

import { describe, expect, it } from 'vitest'

import {
  SIDEBAR_STORAGE_KEY,
  isSidebarShortcut,
  isNewThreadShortcut,
  newThreadShortcut,
  parseSidebarCollapsed,
  serializeSidebarCollapsed,
  sidebarShortcut,
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
