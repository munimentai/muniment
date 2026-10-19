// What the command palette hands core and how its rows read. Core's
// `palette::rank` finds and orders the results. This module gathers the
// window's threads and Settings pages for it, and names each shortcut for the
// platform.
import { settingsIndex, settingsSections } from '../lib/settings-sections.js'

export function paletteShortcut(platform = navigator.platform) {
  return platform.startsWith('Mac') ? 'Meta+K' : 'Control+K'
}

export function isPaletteShortcut(event, platform = navigator.platform) {
  const mac = platform.startsWith('Mac')
  return event.key?.toLowerCase() === 'k'
    && (mac ? event.metaKey && !event.ctrlKey : event.ctrlKey && !event.metaKey)
    && !event.altKey
    && !event.shiftKey
}

// A core shortcut such as `mod+shift+u` as the platform writes it: `⌘⇧U` on
// macOS and `Ctrl+Shift+U` elsewhere.
export function shortcutLabel(shortcut, platform = navigator.platform) {
  if (!shortcut) return ''
  const mac = platform.startsWith('Mac')
  const names = mac
    ? { mod: '⌘', shift: '⇧', alt: '⌥', tab: 'Tab' }
    : { mod: 'Ctrl', shift: 'Shift', alt: 'Alt', tab: 'Tab' }
  const parts = shortcut.split('+').map((part) => names[part] ?? part.toUpperCase())
  return parts.join(mac ? '' : '+')
}

// A core shortcut as `aria-keyshortcuts` names it, such as `Meta+Shift+U`.
export function ariaShortcut(shortcut, platform = navigator.platform) {
  if (!shortcut) return undefined
  const mod = platform.startsWith('Mac') ? 'Meta' : 'Control'
  return shortcut.split('+').map((part) => part === 'mod' ? mod : part === 'shift' ? 'Shift' : part === 'alt' ? 'Alt' : part === 'tab' ? 'Tab' : part.toUpperCase()).join('+')
}

// The Settings pages and parts as palette pages, each part under its page.
// Their ids start with `settings:`, so a part never reads as an app page.
export const SETTINGS_PAGE = 'settings:'
export function settingsPages(options) {
  const sections = settingsSections()
  const titles = Object.fromEntries(sections.map(([id, title]) => [id, title]))
  const icons = Object.fromEntries(sections.map(([id, , icon]) => [id, icon]))
  const pages = sections.map(([id, title, icon]) => ({ id: `${SETTINGS_PAGE}${id}`, title, detail: 'Settings', keywords: [], icon }))
  const parts = settingsIndex(options)
    .filter(([page, , label]) => label !== titles[page])
    .map(([page, part, title, keywords]) => ({ id: `${SETTINGS_PAGE}${part}`, title, detail: `Settings · ${titles[page]}`, keywords: keywords.split(/\s+/), icon: icons[page] ?? 'settings' }))
  return [...pages, ...parts]
}

const thread = (row, positions) => ({
  thread_id: row.thread_id,
  title: row.title,
  project_name: row.project_name,
  status_label: row.status_label,
  updated_at: row.updated_at,
  ...(positions.get(row.thread_id) ? { shortcut: `mod+${positions.get(row.thread_id)}` } : {}),
})

// Core's input from the sidebar's arrangement. `positions` maps a thread id
// to its row shortcut number, one to nine. Archived threads stay out.
export function paletteInput({ query = '', sidebar, positions = new Map(), pages = [], thread: open = null, machines = 1, modes = false }) {
  const rows = sidebar.projects.flatMap((project) => project.threads)
  return {
    query,
    needs_you: rows.filter((row) => row.status === 'needs_you').map((row) => thread(row, positions)),
    threads: rows.map((row) => thread(row, positions)),
    pages,
    thread: open,
    machines,
    modes,
  }
}

export function rankPalette(tauri, input) {
  return tauri.invoke('palette_rank', { input })
}
