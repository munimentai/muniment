// Reads the visible page as numbered elements for the browser tool. An element
// keeps its number while the document lives, so a later action finds it again.
(() => {
  const W = innerWidth, H = innerHeight
  const clip = (text, n) => (text || '').replace(/\s+/g, ' ').trim().slice(0, n)
  const textOf = e => e && (e.innerText ?? e.textContent)
  const shown = e => {
    const r = e.getBoundingClientRect()
    if (r.width < 1 || r.height < 1 || r.bottom < 0 || r.right < 0 || r.top > H || r.left > W) return false
    const s = getComputedStyle(e)
    if (s.visibility === 'hidden' || s.display === 'none' || Number(s.opacity) === 0) return false
    if (e.closest('[aria-hidden="true"],[inert]')) return false
    // An element under a dialog or banner cannot take a click.
    const x = Math.min(Math.max(r.left + r.width / 2, 0), W - 1), y = Math.min(Math.max(r.top + r.height / 2, 0), H - 1)
    const top = document.elementFromPoint(x, y)
    return !top || top === e || e.contains(top) || top.contains(e) || (e.labels && [...e.labels].some(l => l.contains(top)))
  }
  const editable = e => e.isContentEditable || e.tagName === 'TEXTAREA' || ['textbox', 'searchbox'].includes(e.getAttribute('role'))
    || (e.tagName === 'INPUT' && !['button', 'submit', 'reset', 'checkbox', 'radio', 'file', 'image', 'range', 'color', 'hidden', 'password'].includes(e.type))
  const roleOf = e => e.getAttribute('role') || ({ A: 'link', BUTTON: 'button', SELECT: 'combobox', TEXTAREA: 'textbox', SUMMARY: 'button' })[e.tagName]
    || (e.tagName === 'INPUT' ? ({ checkbox: 'checkbox', radio: 'radio', submit: 'button', button: 'button', reset: 'button', search: 'searchbox' })[e.type] || 'textbox' : 'generic')
  const nameOf = e => {
    const by = e.getAttribute('aria-labelledby')
    const labelled = by && by.split(/\s+/).map(id => textOf(document.getElementById(id))).filter(Boolean).join(' ')
    return clip(e.getAttribute('aria-label') || labelled || (e.labels && textOf(e.labels[0])) || e.getAttribute('placeholder')
      || (e.tagName === 'INPUT' ? e.value : textOf(e)) || e.getAttribute('title') || e.getAttribute('alt') || e.querySelector('img[alt]')?.alt, 80)
  }
  const query = 'a[href],button,input,select,textarea,summary,[contenteditable=""],[contenteditable="true"],[tabindex]:not([tabindex="-1"]),'
    + '[role=button],[role=link],[role=checkbox],[role=radio],[role=switch],[role=tab],[role=menuitem],[role=option],[role=combobox],[role=textbox],[role=searchbox],[onclick]'
  const elements = []
  for (const e of document.querySelectorAll(query)) {
    if (elements.length >= 150) break
    if (e.disabled || e.getAttribute('aria-disabled') === 'true' || (e.tagName === 'INPUT' && e.type === 'hidden') || !shown(e)) continue
    // A control inside another listed control is the same target.
    if (e.parentElement?.closest(query) && !editable(e) && e.tagName !== 'SELECT') continue
    const password = e.tagName === 'INPUT' && e.type === 'password'
    window.__munimentNext = window.__munimentNext || 0
    if (!e.dataset.munimentId) e.dataset.munimentId = String(++window.__munimentNext)
    const item = { id: e.dataset.munimentId, role: password ? 'password' : roleOf(e), name: nameOf(e) }
    if (e.tagName === 'SELECT') {
      item.value = clip(e.selectedOptions[0]?.text, 60)
      item.options = [...e.options].slice(0, 30).map((o, i) => ({ index: String(i + 1), label: clip(o.text, 60) }))
      item.operations = ['select']
    } else if (password) {
      item.operations = []
    } else if (editable(e)) {
      item.value = clip(e.isContentEditable ? textOf(e) : e.value, 60)
      item.operations = ['fill']
    } else {
      item.operations = ['click']
    }
    if (e.type === 'checkbox' || e.type === 'radio') item.checked = e.checked
    else if (e.hasAttribute('aria-checked')) item.checked = e.getAttribute('aria-checked') === 'true'
    if (e.hasAttribute('aria-expanded')) item.expanded = e.getAttribute('aria-expanded') === 'true'
    if (e.hasAttribute('aria-selected')) item.selected = e.getAttribute('aria-selected') === 'true'
    elements.push(item)
  }
  // Only text on screen, so long articles and footers stay out of the model's context.
  const parts = []
  let size = 0
  const walker = document.createTreeWalker(document.body || document.documentElement, NodeFilter.SHOW_TEXT)
  for (let node = walker.nextNode(); node && size < 4000; node = walker.nextNode()) {
    const text = clip(node.nodeValue, 400)
    const parent = node.parentElement
    if (!text || !parent || ['SCRIPT', 'STYLE', 'NOSCRIPT'].includes(parent.tagName)) continue
    const r = parent.getBoundingClientRect()
    if (r.bottom < 0 || r.top > H || r.width < 1) continue
    parts.push(text)
    size += text.length + 1
  }
  return { url: location.href, title: document.title, text: parts.join(' '), elements }
})()
