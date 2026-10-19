import { Window, getCurrentWindow } from '@tauri-apps/api/window'
import { PhysicalPosition, LogicalSize } from '@tauri-apps/api/dpi'
import { listen } from '@tauri-apps/api/event'
import { currentZoom } from './display-state.js'
import { followPointer } from './menu-pointer.js'

// Whether a pointer press opened the menu, so the popup marks focus as this window does.
export const pointerLeads = () => document.documentElement.hasAttribute('data-pointer')

// Keep action handlers in the main window; render their menu above native webviews.
export function floatingMenu(node) {
  const pointer = followPointer(node)
  if (!window.__TAURI_INTERNALS__) return pointer
  const id = crypto.randomUUID()
  let popup, disposed = false, floating = false
  const stops = []
  const dismiss = () => {
    const event = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    event.preventDefault()
    node.dispatchEvent(event)
  }
  const retain = off => { if (disposed) off(); else stops.push(off) }
  const buttons = () => [...node.querySelectorAll('button')]
  async function open() {
    if (node.getAttribute('role') !== 'menu') return
    try {
      popup = await Window.getByLabel('workspace-menu')
      if (!popup || disposed) return
      const main = getCurrentWindow()
      const [origin, scale] = await Promise.all([main.innerPosition(), main.scaleFactor()])
      if (disposed) return
      const rect = node.getBoundingClientRect()
      const items = buttons().map((button, index) => ({ id: index, name: button.textContent.trim(), disabled: button.disabled, icon: button.querySelector('svg')?.getAttribute('data-icon') }))
      const width = Math.min(320, Math.max(180, rect.width + 16))
      // The page measures CSS pixels, and the interface zoom scales them to window pixels.
      const zoom = currentZoom()
      const height = Math.min(innerHeight * zoom, items.length * 36 + 30)
      await popup.setSize(new LogicalSize(width, height))
      await popup.setPosition(new PhysicalPosition(Math.round(origin.x + Math.max(0, Math.min(rect.left * zoom - 8, innerWidth * zoom - width)) * scale), Math.round(origin.y + Math.max(0, Math.min(rect.top * zoom - 8, innerHeight * zoom - height)) * scale)))
      if (disposed) return
      floating = true
      node.dataset.floatingMenu = 'true'
      node.style.opacity = '0'
      node.style.pointerEvents = 'none'
      await popup.emit('action-menu-open', { id, label: node.getAttribute('aria-label'), items, pointer: pointerLeads() })
    } catch { restore() }
  }
  function restore() { floating = false; delete node.dataset.floatingMenu; node.style.removeProperty('opacity'); node.style.removeProperty('pointer-events') }
  const observer = new MutationObserver(() => {
    if (floating && node.getAttribute('role') !== 'menu') { restore(); void popup?.hide() }
  })
  observer.observe(node, { attributes: true, attributeFilter: ['role'] })
  void (async () => {
    retain(await listen('action-menu-select', ({ payload }) => {
      if (payload.id !== id || disposed) return
      restore()
      buttons()[payload.item]?.click()
    }))
    retain(await listen('action-menu-closed', ({ payload }) => { if (payload === id && !disposed) { restore(); dismiss() } }))
    if (!disposed) await open()
  })().catch(() => restore())
  return { destroy() { pointer.destroy(); disposed = true; observer.disconnect(); stops.forEach(off => off()); if (floating) void popup?.hide(); restore() } }
}
