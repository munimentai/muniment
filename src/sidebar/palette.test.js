import { describe, expect, it, vi } from 'vitest'
import { ariaShortcut, isPaletteShortcut, paletteInput, paletteShortcut, rankPalette, settingsPages, shortcutLabel } from './palette.js'
import { layerLabel, sourceLine } from './sources.js'
import { emptySidebar } from './sidebar-model.js'

const row = (id, title, extra = {}) => ({ thread_id: id, title, project_id: '', project_name: '', updated_at: '2026-10-18T10:00:00Z', status: 'read', status_label: '', ...extra })

describe('command palette', () => {
  it('takes the plain key on each platform and leaves Shift to the record panel', () => {
    expect(paletteShortcut('MacIntel')).toBe('Meta+K')
    expect(paletteShortcut('Win32')).toBe('Control+K')
    expect(isPaletteShortcut({ key: 'k', metaKey: true, ctrlKey: false, altKey: false, shiftKey: false }, 'MacIntel')).toBe(true)
    expect(isPaletteShortcut({ key: 'K', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false }, 'Linux x86_64')).toBe(true)
    expect(isPaletteShortcut({ key: 'K', metaKey: true, ctrlKey: false, altKey: false, shiftKey: true }, 'MacIntel')).toBe(false)
    expect(isPaletteShortcut({ key: 'k', metaKey: false, ctrlKey: true, altKey: false, shiftKey: false }, 'MacIntel')).toBe(false)
  })

  it('writes core shortcuts for the platform', () => {
    expect(shortcutLabel('mod+shift+u', 'MacIntel')).toBe('⌘⇧U')
    expect(shortcutLabel('mod+shift+u', 'Win32')).toBe('Ctrl+Shift+U')
    expect(shortcutLabel('shift+tab', 'MacIntel')).toBe('⇧Tab')
    expect(shortcutLabel('mod+,', 'Linux x86_64')).toBe('Ctrl+,')
    expect(shortcutLabel(null)).toBe('')
    expect(ariaShortcut('mod+2', 'MacIntel')).toBe('Meta+2')
    expect(ariaShortcut('shift+tab', 'Win32')).toBe('Shift+Tab')
    expect(ariaShortcut(undefined)).toBeUndefined()
  })

  it('offers each Settings page and part under an id of its own', () => {
    const pages = settingsPages()
    expect(pages.find((page) => page.id === 'settings:models')).toMatchObject({ title: 'Models', detail: 'Settings' })
    expect(pages.find((page) => page.id === 'settings:mcp')).toMatchObject({ title: 'MCP servers', detail: 'Settings · Tools' })
    expect(pages.find((page) => page.id === 'settings:notifications')).toMatchObject({ detail: 'Settings · General' })
    // A part named like its page adds no second row.
    expect(pages.filter((page) => page.title === 'Machines')).toHaveLength(1)
  })

  it('hands core every thread the sidebar knows with its row shortcut and leaves archived threads out', () => {
    const sidebar = {
      ...emptySidebar(),
      archived: [row('c', 'Archived one')],
      projects: [
        { id: 'a', name: 'Approve me', kind: 'thread', threads: [row('a', 'Approve me', { status: 'needs_you', status_label: 'Approve' })] },
        { id: 'p1', name: 'Homelab', kind: 'project', threads: [row('b', 'Recent one', { project_id: 'p1', project_name: 'Homelab' }), row('d', 'Folded one', { project_id: 'p1', project_name: 'Homelab' })] },
      ],
    }
    const input = paletteInput({ query: 're', sidebar, positions: new Map([['a', 1], ['b', 2]]), thread: { thread_id: 'b', pinned: false, movable: true }, machines: 2, modes: true })
    expect(input.query).toBe('re')
    expect(input.needs_you).toEqual([{ thread_id: 'a', title: 'Approve me', project_name: '', status_label: 'Approve', updated_at: '2026-10-18T10:00:00Z', shortcut: 'mod+1' }])
    expect(input.threads.map((thread) => thread.thread_id)).toEqual(['a', 'b', 'd'])
    expect(input.threads[1].shortcut).toBe('mod+2')
    expect(input.threads.some((thread) => thread.thread_id === 'c')).toBe(false)
    expect(input).toMatchObject({ machines: 2, modes: true, thread: { thread_id: 'b', movable: true } })
  })

  it('asks core to rank', async () => {
    const tauri = { invoke: vi.fn(async () => []) }
    await rankPalette(tauri, { query: '' })
    expect(tauri.invoke).toHaveBeenCalledWith('palette_rank', { input: { query: '' } })
  })
})

describe('value sources', () => {
  it('words each layer core names', () => {
    expect(layerLabel('thread')).toBe('this thread')
    expect(layerLabel('agent', { agentName: 'Deploy bot' })).toBe('from agent Deploy bot')
    expect(layerLabel('project', { projectName: 'Homelab' })).toBe('from project Homelab')
    expect(layerLabel('default')).toBe('default')
    expect(sourceLine('GPT-6 Astra', 'project', { projectName: 'Homelab' })).toBe('GPT-6 Astra · from project Homelab')
    expect(sourceLine('Bypass permissions', 'default')).toBe('Bypass permissions · default')
    expect(sourceLine('', 'default')).toBe('default')
  })
})
