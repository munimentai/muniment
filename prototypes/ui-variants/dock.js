// Shared harness for the color variants: pins the base scheme and frame,
// widens the sidebar, lifts the variant styles above the bundle, and can open
// Preferences on load.
const { mode, frame, open } = document.documentElement.dataset
try {
  // A variant page pins its base scheme and frame. The preferences page keeps the viewer's picks.
  if (mode) localStorage.setItem('muniment.theme', JSON.stringify({ mode, palette: 'moss' }))
  if (frame) localStorage.setItem('muniment.frame', frame)
  localStorage.setItem('muniment.sidebar-width', '256')
} catch (_) {}

window.__variantReady = async () => {
  await window.__PROBE__.loadBundle()
  // The bundle appends its styles at load; the variant must follow them.
  document.head.append(...document.querySelectorAll('link[data-variant], style[data-variant]'))
  if (open === 'preferences') {
    const until = async (find) => { for (let i = 0; i < 100 && !find(); i++) await new Promise(r => setTimeout(r, 50)); return find() }
    ;(await until(() => document.querySelector('#sidebar .side-foot .side-action')))?.click()
    const rows = '#sidebar nav[aria-label="Settings sections"] button'
    await until(() => document.querySelector(rows))
    // A hash names the page to open, such as preferences.html#Voice.
    const page = decodeURIComponent(location.hash.slice(1)) || 'Appearance'
    ;[...document.querySelectorAll(rows)].find(b => b.textContent.trim() === page)?.click()
  }
}
