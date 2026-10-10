// Shared harness for the color variants: picks the base scheme, widens the
// sidebar, lifts the variant styles above the bundle, and docks Settings into
// the sidebar column with a Back control.
const page = document.currentScript || document.querySelector('script[data-mode]')
const mode = document.documentElement.dataset.mode || 'light'
try {
  localStorage.setItem('muniment.theme', mode === 'dark' ? 'vault' : 'paper')
  localStorage.setItem('muniment.sidebar-width', '256')
} catch (_) {}

const icon = (d) => `<svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${d}</svg>`
const icons = {
  back: icon('<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>'),
  'Models & decisions': icon('<rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6" rx="1"/><path d="M15 2v2M15 20v2M2 15h2M2 9h2M20 15h2M20 9h2M9 2v2M9 20v2"/>'),
  Extend: icon('<path d="M12 22v-5"/><path d="M9 8V2"/><path d="M15 8V2"/><path d="M18 8v5a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V8Z"/>'),
  Preferences: icon('<path d="M20 7h-9"/><path d="M14 17H5"/><circle cx="17" cy="17" r="3"/><circle cx="7" cy="7" r="3"/>'),
  'Profile & Memory': icon('<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>'),
  Storage: icon('<path d="M22 12H2"/><path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/><path d="M6 16h.01M10 16h.01"/>'),
  Companies: icon('<rect x="4" y="2" width="16" height="20" rx="2"/><path d="M9 22v-4h6v4M8 6h.01M16 6h.01M12 6h.01M12 10h.01M12 14h.01M16 10h.01M16 14h.01M8 10h.01M8 14h.01"/>'),
  Account: icon('<circle cx="12" cy="12" r="10"/><circle cx="12" cy="10" r="3"/><path d="M7 20.66V19a2 2 0 0 1 2-2h6a2 2 0 0 1 2 2v1.66"/>'),
}

function dock() {
  const scrim = document.querySelector('.settings-scrim')
  const root = document.documentElement
  if (!scrim) { root.classList.remove('settings-docked'); return }
  const side = document.getElementById('sidebar')
  const width = side ? side.getBoundingClientRect().width : 0
  root.style.setProperty('--dock-side', `${Math.max(width, 240)}px`)
  root.classList.add('settings-docked')
  const nav = scrim.querySelector('.settings-nav')
  if (nav && !nav.querySelector('.dock-back')) {
    const back = document.createElement('button')
    back.type = 'button'
    back.className = 'dock-back'
    back.innerHTML = `${icons.back}<span>Back to threads</span>`
    back.addEventListener('click', () => scrim.querySelector('[aria-label="Close settings"]')?.click())
    nav.prepend(back)
  }
  for (const button of scrim.querySelectorAll('.settings-nav li > button')) {
    if (button.querySelector('svg')) continue
    const svg = icons[button.textContent.trim()]
    if (svg) button.insertAdjacentHTML('afterbegin', svg)
  }
}

addEventListener('DOMContentLoaded', () => {
  new MutationObserver(dock).observe(document.body, { childList: true, subtree: true })
})

window.__variantReady = async () => {
  await window.__PROBE__.loadBundle()
  // The bundle appends its styles at load; the variant must follow them.
  document.head.append(...document.querySelectorAll('link[data-variant], style[data-variant]'))
}
