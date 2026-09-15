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
    expect(shortcutDisplayLabel('Meta+J')).toBe('⌘ J')
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

import {
  RECORD_PANEL_MAX_WIDTH,
  RECORD_PANEL_MIN_WIDTH,
  clampRailWidth,
  createRailController,
  defaultRailWidth,
  isRecordPanelShortcut,
  railWidthFromKey,
  recordPanelShortcut,
} from './artifact-rail-state.js'

describe('the rail column with two occupants', () => {
  it('gives the record panel its own bounds and default share', () => {
    expect(clampRailWidth(300, 'record')).toBe(RECORD_PANEL_MIN_WIDTH)
    expect(clampRailWidth(2000, 'record')).toBe(RECORD_PANEL_MAX_WIDTH)
    expect(clampRailWidth(700, 'record', 600)).toBe(600)
    expect(defaultRailWidth(1400, 'record')).toBe(700)
    expect(defaultRailWidth(1400, 'artifacts')).toBe(476)
    expect(railWidthFromKey(500, 'Home', 'record')).toBe(RECORD_PANEL_MIN_WIDTH)
    expect(railWidthFromKey(500, 'End', 'record')).toBe(RECORD_PANEL_MAX_WIDTH)
  })

  it('names the record shortcut per platform and recognizes it', () => {
    expect(recordPanelShortcut('MacIntel')).toBe('Meta+K')
    expect(recordPanelShortcut('Win32')).toBe('Control+K')
    expect(isRecordPanelShortcut({ key: 'k', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }, 'MacIntel')).toBe(true)
    expect(isRecordPanelShortcut({ key: 'K', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false }, 'Linux')).toBe(true)
    expect(isRecordPanelShortcut({ key: 'k', metaKey: true, ctrlKey: false, altKey: false, shiftKey: true }, 'MacIntel')).toBe(false)
    expect(isRecordPanelShortcut({ key: 'j', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }, 'MacIntel')).toBe(false)
  })

  it('opens one occupant at a time, resets the width per occupant, and maximizes the record alone', () => {
    const state = { occupant: null, width: 0, maximum: 0, pointer: undefined, maximized: false }
    const controller = createRailController({
      readOccupant: () => state.occupant,
      onOccupant: (next) => { state.occupant = next },
      readWidth: () => state.width,
      onWidth: (next) => { state.width = next },
      readMaximum: () => state.maximum,
      onMaximum: (next) => { state.maximum = next },
      readPointer: () => state.pointer,
      onPointer: (next) => { state.pointer = next },
      readAvailableWidth: (occupant) => (occupant === 'record' ? 800 : 500),
      readRightEdge: () => 1400,
      readViewportWidth: () => 1400,
      readMaximized: () => state.maximized,
      onMaximized: (next) => { state.maximized = next },
    })

    controller.toggle('artifacts')
    expect(state.occupant).toBe('artifacts')
    expect(state.width).toBe(476)
    expect(state.maximum).toBe(500)

    controller.toggleMaximized()
    expect(state.maximized).toBe(false)

    controller.toggle('record')
    expect(state.occupant).toBe('record')
    expect(state.width).toBe(700)
    expect(state.maximum).toBe(800)

    controller.toggleMaximized()
    expect(state.maximized).toBe(true)
    controller.toggle('record')
    expect(state.occupant).toBeNull()
    expect(state.maximized).toBe(false)

    controller.open('record')
    controller.toggleMaximized()
    controller.toggle('artifacts')
    expect(state.occupant).toBe('artifacts')
    expect(state.maximized).toBe(false)
    controller.close()
    expect(state.occupant).toBeNull()
  })
})
