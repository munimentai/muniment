<script>
  import { onDestroy } from 'svelte'
  import { Menu } from '@tauri-apps/api/menu'
  import { LogicalPosition } from '@tauri-apps/api/dpi'
  import { save } from '@tauri-apps/plugin-dialog'
  import { openUrl } from '@tauri-apps/plugin-opener'
  import { renderAssistantMarkdown } from './assistant-markdown.js'
  import { createExternalLinkHandler } from './external-link.js'
  import { scrollRegionOverflows } from './scroll-region.js'

  // `caret` draws the active-line caret at the end of the last text block, so a
  // reply renders as Markdown from its first token with the caret inside it.
  let { text = '', caret = false, onopenlink, tauri } = $props()
  let rendered = $derived(renderAssistantMarkdown(text))
  let container
  let caretElement
  let linkStatus = $state('')
  let linkMenu
  const openInBrowser = address => /^https?:/i.test(address) && onopenlink ? onopenlink(address) : openUrl(address)
  const handleExternalLink = createExternalLinkHandler(openInBrowser)
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
  onDestroy(() => { void linkMenu?.close().catch(() => {}) })

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

  function updateScrollRegions() {
    for (const element of container?.querySelectorAll('pre, table') ?? []) {
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
  }

  $effect(() => {
    void rendered
    void caret
    placeCaret()
    updateScrollRegions()

    if (!container || typeof ResizeObserver === 'undefined') return

    const observer = new ResizeObserver(updateScrollRegions)
    observer.observe(container)
    return () => observer.disconnect()
  })
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (The handler delegates native link activation.) -->
<div class="assistant-markdown" onclick={handleExternalLink} oncontextmenu={linkContextMenu} bind:this={container}>
  {#if rendered.kind === 'html'}
    {@html rendered.html}
  {:else}{rendered.text}{/if}
</div>
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
