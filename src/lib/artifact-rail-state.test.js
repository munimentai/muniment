// @vitest-environment jsdom

import { describe, expect, it } from 'vitest'

import {
  ARTIFACT_RAIL_MAX_WIDTH,
  ARTIFACT_RAIL_MIN_WIDTH,
  artifactRailShortcut,
  artifactRailWidthFromKey,
  artifactRailWidthFromPointer,
  clampArtifactRailWidth,
  defaultArtifactRailWidth,
  isArtifactRailShortcut,
  shortcutDisplayLabel,
} from './artifact-rail-state.js'

describe('artifact rail state', () => {
  it('clamps rail widths to the design bounds and the available layout', () => {
    expect(clampArtifactRailWidth(200)).toBe(ARTIFACT_RAIL_MIN_WIDTH)
    expect(clampArtifactRailWidth(700)).toBe(ARTIFACT_RAIL_MAX_WIDTH)
    expect(clampArtifactRailWidth(540, 480)).toBe(480)
    expect(clampArtifactRailWidth(Number.NaN)).toBe(ARTIFACT_RAIL_MIN_WIDTH)
    expect(defaultArtifactRailWidth(1400)).toBe(476)
  })

  it('derives a clamped rail width from the divider position', () => {
    expect(artifactRailWidthFromPointer(900, 1400)).toBe(500)
    expect(artifactRailWidthFromPointer(1200, 1400)).toBe(ARTIFACT_RAIL_MIN_WIDTH)
    expect(artifactRailWidthFromPointer(700, 1400)).toBe(ARTIFACT_RAIL_MAX_WIDTH)
    expect(artifactRailWidthFromPointer(850, 1400, 480)).toBe(480)
  })

  it('supports window-splitter keys in consistent increments', () => {
    expect(artifactRailWidthFromKey(440, 'ArrowLeft')).toBe(460)
    expect(artifactRailWidthFromKey(440, 'ArrowRight')).toBe(420)
    expect(artifactRailWidthFromKey(440, 'Home')).toBe(ARTIFACT_RAIL_MIN_WIDTH)
    expect(artifactRailWidthFromKey(440, 'End')).toBe(ARTIFACT_RAIL_MAX_WIDTH)
    expect(artifactRailWidthFromKey(440, 'End', 500)).toBe(500)
    expect(artifactRailWidthFromKey(440, 'Enter')).toBe(440)
  })

  it('uses the platform primary modifier', () => {
    expect(artifactRailShortcut('MacIntel')).toBe('Meta+J')
    expect(artifactRailShortcut('Win32')).toBe('Control+J')
    expect(artifactRailShortcut('Linux x86_64')).toBe('Control+J')
  })

  it('formats platform shortcuts for display', () => {
    expect(shortcutDisplayLabel('Meta+J')).toBe('⌘J')
    expect(shortcutDisplayLabel('Control+J')).toBe('Ctrl J')
  })

  it('recognizes only the platform shortcut without extra modifiers', () => {
    expect(isArtifactRailShortcut(new KeyboardEvent('keydown', { key: 'j', metaKey: true }), 'MacIntel')).toBe(true)
    expect(isArtifactRailShortcut(new KeyboardEvent('keydown', { key: 'J', ctrlKey: true }), 'Linux x86_64')).toBe(true)
    expect(isArtifactRailShortcut(new KeyboardEvent('keydown', { key: 'j', ctrlKey: true }), 'MacIntel')).toBe(false)
    expect(isArtifactRailShortcut(new KeyboardEvent('keydown', { key: 'j', ctrlKey: true, shiftKey: true }), 'Win32')).toBe(false)
  })

  it('recognizes shortcuts from editable controls', () => {
    for (const target of [document.createElement('input'), document.createElement('textarea')]) {
      expect(isArtifactRailShortcut({ key: 'j', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, target }, 'Win32')).toBe(true)
    }
    const editable = document.createElement('div')
    editable.setAttribute('contenteditable', 'true')
    expect(isArtifactRailShortcut({ key: 'j', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, target: editable }, 'Win32')).toBe(true)
  })
})
