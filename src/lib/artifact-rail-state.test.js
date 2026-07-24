// @vitest-environment jsdom

import { describe, expect, it } from 'vitest'

import { artifactRailShortcut, isArtifactRailShortcut, isEditableTarget } from './artifact-rail-state.js'

describe('artifact rail state', () => {
  it('uses the platform primary modifier', () => {
    expect(artifactRailShortcut('MacIntel')).toBe('Meta+J')
    expect(artifactRailShortcut('Win32')).toBe('Control+J')
    expect(artifactRailShortcut('Linux x86_64')).toBe('Control+J')
  })

  it('recognizes only the platform shortcut without extra modifiers', () => {
    expect(isArtifactRailShortcut(new KeyboardEvent('keydown', { key: 'j', metaKey: true }), 'MacIntel')).toBe(true)
    expect(isArtifactRailShortcut(new KeyboardEvent('keydown', { key: 'J', ctrlKey: true }), 'Linux x86_64')).toBe(true)
    expect(isArtifactRailShortcut(new KeyboardEvent('keydown', { key: 'j', ctrlKey: true }), 'MacIntel')).toBe(false)
    expect(isArtifactRailShortcut(new KeyboardEvent('keydown', { key: 'j', ctrlKey: true, shiftKey: true }), 'Win32')).toBe(false)
  })

  it('suppresses shortcuts from editable controls', () => {
    for (const target of [document.createElement('input'), document.createElement('textarea')]) {
      expect(isEditableTarget(target)).toBe(true)
      expect(isArtifactRailShortcut({ key: 'j', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, target }, 'Win32')).toBe(false)
    }
    const editable = document.createElement('div')
    editable.setAttribute('contenteditable', 'true')
    document.body.append(editable)
    expect(isEditableTarget(editable)).toBe(true)
  })
})
