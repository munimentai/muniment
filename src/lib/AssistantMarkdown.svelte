<script>
  import { onDestroy, onMount, untrack } from 'svelte'
  import { Menu } from '@tauri-apps/api/menu'
  import { LogicalPosition } from '@tauri-apps/api/dpi'
  import { save } from '@tauri-apps/plugin-dialog'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { renderAssistantMarkdownBlocks } from './assistant-markdown.js'
  import { createExternalLinkHandler } from './external-link.js'
  import { scrollRegionOverflows } from './scroll-region.js'

  // `caret` draws the active-line caret at the end of the last text block, so a
  // reply renders as Markdown from its first token with the caret inside it.
  let { text = '', caret = false, onopenlink, tauri } = $props()
  let container
  let caretElement
  let linkStatus = $state('')
  let linkMenu
  const openInBrowser = address => /^https?:/i.test(address) && onopenlink ? onopenlink(address) : openUrl(address)
  const handleExternalLink = createExternalLinkHandler(openInBrowser)
  // A middle click opens the link like a click. A right click opens the link menu.
  const handleAuxLink = event => { if (event.button === 1) return handleExternalLink(event) }
  async function linkAction(action) {
    linkStatus = ''
    try { await action() } catch (error) { linkStatus = String(error?.message ?? error) }
  }
  async function saveLink(url) {
    const pathname = new URL(url).pathname
    let filename = pathname.split('/').at(-1) || 'index.html'
    try { filename = decodeURIComponent(filename) } catch { /* Keep the URL filename. */ }
    filename = filename.replace(/[\\/:*?"<>|]/g, '_')
    const path = await save({defaultPath:filename})
    if (!path) return
    linkStatus = 'Saving link…'
    await tauri.invoke('workspace_save_link',{url,path})
    linkStatus = 'Link saved.'
  }
  async function linkContextMenu(event) {
    const link = event.target?.closest?.('a[href]')
    if (!link || !container.contains(link)) return
    const address = link.getAttribute('href')
    let url
    try { url = new URL(address) } catch { return }
    if (!['http:','https:','mailto:'].includes(url.protocol)) return
    event.preventDefault()
    const rect = link.getBoundingClientRect()
    const position = new LogicalPosition(event.clientX || rect.left, event.clientY || rect.bottom)
    await linkAction(async () => {
      await linkMenu?.close()
      linkMenu = await Menu.new({items:[
        {text:'Open in browser',enabled:url.protocol !== 'mailto:',action:() => linkAction(() => openInBrowser(address))},
        {text:'Open in external browser',action:() => linkAction(() => openUrl(address))},
        {item:'Separator'},
        {text:'Copy link',action:() => linkAction(async () => { await navigator.clipboard.writeText(address); linkStatus = 'Link copied.' })},
        {text:'Save link as…',enabled:!!tauri && url.protocol !== 'mailto:',action:() => linkAction(() => saveLink(address))},
      ]})
      await linkMenu.popup(position)
    })
  }
  const nextFrame = callback => typeof requestAnimationFrame === 'function' ? requestAnimationFrame(callback) : setTimeout(callback, 16)
  const cancelFrame = id => typeof cancelAnimationFrame === 'function' ? cancelAnimationFrame(id) : clearTimeout(id)
  let drawFrame = 0
  let regionFrame = 0
  onDestroy(() => {
    void linkMenu?.close().catch(() => {})
    if (drawFrame) cancelFrame(drawFrame)
    if (regionFrame) cancelFrame(regionFrame)
  })

  // The block that holds the reply's last text: descend through the last child
  // while it is a block that takes inline content, and stop above a code block,
  // a table or a rule, which take none.
  const DESCEND = new Set(['P', 'LI', 'UL', 'OL', 'BLOCKQUOTE', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6'])
  // Svelte's anchor comments, blank text and the caret itself are not content.
  const skipped = (node) => node.nodeType === Node.COMMENT_NODE
    || (node.nodeType === Node.TEXT_NODE && !node.textContent.trim())
    || node === caretElement
  export function caretHost(root) {
    let host = root
    for (;;) {
      let last = host.lastChild
      while (last && skipped(last)) last = last.previousSibling
      if (!last || last.nodeType !== Node.ELEMENT_NODE || !DESCEND.has(last.tagName)) return host
      host = last
    }
  }

  function placeCaret() {
    if (!container) return
    if (!caret) {
      caretElement?.remove()
      caretElement = undefined
      return
    }
    if (!caretElement) {
      caretElement = document.createElement('span')
      caretElement.className = 'caret'
      caretElement.textContent = '_'
      caretElement.setAttribute('aria-hidden', 'true')
    }
    caretHost(container).appendChild(caretElement)
  }

  function updateScrollRegion(element) {
    const overflows = scrollRegionOverflows({
      scrollWidth: element.scrollWidth,
      clientWidth: element.clientWidth,
    })

    if (overflows) {
      element.setAttribute('tabindex', '0')
      element.setAttribute('role', 'group')
      element.setAttribute('aria-label', element.tagName === 'PRE' ? 'Code block' : 'Table')
    } else {
      element.removeAttribute('tabindex')
      element.removeAttribute('role')
      element.removeAttribute('aria-label')
    }
  }

  // The overflow check reads layout, so it waits for the next frame. A width
  // change checks every code block and table. New blocks check only their own.
  let pendingRegions = new Set()
  let allRegions = false
  function queueScrollRegions(nodes) {
    if (nodes) {
      for (const node of nodes) {
        if (node.nodeType !== Node.ELEMENT_NODE) continue
        if (node.matches('pre, table')) pendingRegions.add(node)
        for (const element of node.querySelectorAll('pre, table')) pendingRegions.add(element)
      }
      if (!pendingRegions.size) return
    } else {
      allRegions = true
    }
    regionFrame ||= nextFrame(checkScrollRegions)
  }
  function checkScrollRegions() {
    regionFrame = 0
    const elements = allRegions ? container?.querySelectorAll('pre, table') ?? [] : [...pendingRegions]
    allRegions = false
    pendingRegions = new Set()
    for (const element of elements) if (element.isConnected) updateScrollRegion(element)
  }

  // Each top-level block keeps the DOM nodes it inserted. A redraw keeps every
  // leading block the renderer reused and replaces the blocks after it, so text
  // selected in a finished block stays selected while the reply streams.
  let result = null
  let mounted = []
  let plainText = null
  let drawnText
  function draw() {
    drawFrame = 0
    if (!container) return
    const next = renderAssistantMarkdownBlocks(text, { previous: result })
    drawnText = text
    caretElement?.remove()
    if (next.kind === 'text') {
      for (const { nodes } of mounted) for (const node of nodes) node.remove()
      mounted = []
      plainText ??= container.appendChild(document.createTextNode(''))
      plainText.data = next.text
    } else {
      plainText?.remove()
      plainText = null
      let kept = 0
      while (kept < mounted.length && mounted[kept].block === next.blocks[kept]) kept += 1
      for (const { nodes } of mounted.splice(kept)) for (const node of nodes) node.remove()
      const inserted = []
      for (const block of next.blocks.slice(kept)) {
        const fragment = block.fragment ?? blockFragment(block.html)
        block.fragment = null
        const nodes = [...fragment.childNodes]
        container.appendChild(fragment)
        mounted.push({ block, nodes })
        inserted.push(...nodes)
      }
      queueScrollRegions(inserted)
    }
    result = next
    placeCaret()
  }
  function blockFragment(html) {
    const template = document.createElement('template')
    template.innerHTML = html
    return template.content
  }

  // The first draw is immediate. Later text draws once per animation frame.
  $effect(() => {
    const current = text
    void caret
    untrack(() => {
      if (drawnText === undefined) draw()
      else if (current === drawnText && !drawFrame) placeCaret()
      else drawFrame ||= nextFrame(draw)
    })
  })

  onMount(() => {
    if (typeof ResizeObserver === 'undefined') return
    let width
    const observer = new ResizeObserver(entries => {
      const next = entries?.[0]?.contentRect?.width
      if (next !== undefined && next === width) return
      width = next
      queueScrollRegions(null)
    })
    observer.observe(container)
    return () => observer.disconnect()
  })
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (The handler delegates native link activation.) -->
<div class="assistant-markdown" onclick={handleExternalLink} onauxclick={handleAuxLink} oncontextmenu={linkContextMenu} bind:this={container}></div>
{#if linkStatus}<p class="link-status" role="status">{linkStatus}</p>{/if}

<style>
  .link-status { color:var(--muted); font:var(--text-12) var(--font-mono); margin-top:6px; }
  .assistant-markdown { color: var(--ink); white-space: pre-wrap; overflow-wrap: anywhere; }
  .assistant-markdown :global(p) { margin: 0 0 12px; }
  .assistant-markdown :global(p:last-child) { margin-bottom: 0; }
  .assistant-markdown :global(h2) { margin: 24px 0 8px; font-size: var(--text-22); }
  .assistant-markdown :global(h3) { margin: 20px 0 7px; font-size: var(--text-17); }
  .assistant-markdown :global(h4), .assistant-markdown :global(h5) { margin: 16px 0 6px; font-size: var(--text-15); }
  .assistant-markdown :global(h2:first-child), .assistant-markdown :global(h3:first-child), .assistant-markdown :global(h4:first-child), .assistant-markdown :global(h5:first-child) { margin-top: 0; }
  /* Two-digit list markers sit inside this padding, so a scrolling thread never clips them. */
  .assistant-markdown :global(ul), .assistant-markdown :global(ol) { margin: 0 0 12px; padding-left: 28px; }
  .assistant-markdown :global(.caret) { display: inline-block; color: var(--signal); font-family: var(--font-mono); margin-left: 2px; animation: blink 800ms step-end infinite; }
  @keyframes blink { 50% { opacity: 0; } }
  .assistant-markdown :global(li > p) { margin-bottom: 4px; }
  .assistant-markdown :global(blockquote) { margin: 0 0 12px; padding-left: 12px; border-left: 1px solid var(--border); color: var(--muted); }
  .assistant-markdown :global(hr) { margin: 20px 0; border: 0; border-top: 1px solid var(--border); }
  .assistant-markdown :global(a) { color: var(--ink); text-decoration: underline; text-decoration-color: var(--border); text-underline-offset: 2px; }
  .assistant-markdown :global(a:hover) { color: var(--muted); text-decoration-color: var(--muted); }
  .assistant-markdown :global(code) { padding: 1px 3px; border-radius: var(--radius-chip); background: var(--faint); font: var(--text-13) var(--font-mono); }
  .assistant-markdown :global(pre) { max-width: 100%; margin: 0 0 12px; overflow-x: auto; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--faint); }
  .assistant-markdown :global(pre code) { display: block; width: max-content; min-width: 100%; padding: 10px 12px; border-radius: var(--radius-control); background: var(--faint); white-space: pre; }
  .assistant-markdown :global(table) { display: block; max-width: 100%; margin: 0 0 12px; overflow-x: auto; border-collapse: collapse; white-space: nowrap; }
  .assistant-markdown :global(th), .assistant-markdown :global(td) { padding: 6px 10px; border: 1px solid var(--border); text-align: left; }
  .assistant-markdown :global(th) { background: var(--faint); font-weight: var(--weight-semibold); }
</style>
