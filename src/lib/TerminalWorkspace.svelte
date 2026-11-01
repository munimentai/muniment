<script>
  import { onMount, tick } from 'svelte'
  import { Terminal } from '@xterm/xterm'
  import { FitAddon } from '@xterm/addon-fit'
  import '@xterm/xterm/css/xterm.css'
  import { TYPE_EVENT, codeFontSize } from './type-state.js'
  import { inputMarkSettler } from './focus-landing.js'
  let { tauri, context = null, hidden = false, path = '', threadId = null, projectId = null, oncwd } = $props()
  let host
  let terminal
  let fit
  let id = null
  let cwd = $state('')
  let error = $state('')
  let exited = $state(false)
  let starting = $state(false)
  let active = true
  let timer
  let inputQueue = Promise.resolve()
  // The shell hides the input-source mark over the prompt while focus, typing or output moves the caret.
  const inputMark = inputMarkSettler((command) => tauri?.invoke(command))
  // A folder path too long for the line drops its start behind a leading
  // ellipsis, as the Files panel does, and only then names the full path on hover.
  let pathCut = $state(false)
  function cutStart(node) {
    const measure = () => { pathCut = node.scrollWidth > node.clientWidth }
    const resize = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure)
    resize?.observe(node)
    measure()
    return { update: measure, destroy: () => resize?.disconnect() }
  }
  async function resize() {
    if (hidden || !terminal || !id) return
    fit.fit()
    try { await tauri.invoke('terminal_resize', { id, cols: terminal.cols, rows: terminal.rows }) } catch (e) { if (active) error = String(e) }
  }
  async function poll() {
    if (!active || !id) return
    try {
      const result = await tauri.invoke('terminal_read', { id })
      if (!active) return
      if (result.bytes.length) terminal.write(new Uint8Array(result.bytes))
      exited = result.exited
      if (exited) return
    } catch (e) { if (active) error = String(e); return }
    timer = setTimeout(poll,100)
  }
  function theme() {
    if (!terminal || !host) return
    const style = getComputedStyle(host)
    terminal.options.theme = { selectionBackground: '#FFD84D', selectionInactiveBackground: '#FFD84D', selectionForeground: '#171A18', background: style.getPropertyValue('--paper').trim(), foreground: style.getPropertyValue('--ink').trim(), cursor: style.getPropertyValue('--ink').trim() }
  }
  async function start() {
    if (starting) return
    starting = true
    error = ''; exited = false
    try {
      if (id) await tauri.invoke('terminal_close', { id })
      id = null; clearTimeout(timer); terminal.reset()
      const folders = await tauri.invoke('workspace_folders', context || { threadId, projectId })
      if (!active) return
      cwd = path || folders.at(-1)?.path || ''
      oncwd?.(cwd)
      if (!cwd) throw new Error('Choose a workspace folder first.')
      fit.fit()
      const next = await tauri.invoke('terminal_start', { path: cwd, cols: terminal.cols, rows: terminal.rows })
      if (!active) { await tauri.invoke('terminal_close', { id: next }); return }
      id = next; if (!hidden) terminal.focus(); void poll()
      // The panel can grow while the shell starts, as it slides open, and a
      // shell told a width too narrow for its prompt draws none. The shell
      // takes the panel's size as soon as it has an id.
      await resize()
    } catch (e) { if (active) error = String(e) }
    finally { starting = false }
  }
  $effect(() => { if (!hidden) void tick().then(() => { theme(); void resize() }) })
  onMount(() => {
    terminal = new Terminal({ fontFamily: 'Commit Mono, monospace', fontSize: codeFontSize(), cursorBlink: true, scrollback: 5000, allowProposedApi: false })
    fit = new FitAddon(); terminal.loadAddon(fit); terminal.open(host); theme()
    // A focused terminal keeps ⌘K on macOS to clear itself; elsewhere Ctrl+K reaches the shell.
    const mac = navigator.platform.startsWith('Mac')
    terminal.attachCustomKeyEventHandler((event) => {
      if (!mac || event.type !== 'keydown' || event.key.toLowerCase() !== 'k' || !event.metaKey || event.shiftKey || event.altKey || event.ctrlKey) return true
      terminal.clear()
      return false
    })
    const input = terminal.onData(data => { inputMark.poke(); if (!id || exited) return; const session = id; inputQueue = inputQueue.then(() => tauri.invoke('terminal_write', { id: session, data })).catch(e => { if (active) error = String(e) }) })
    const field = terminal.textarea; field?.addEventListener('focus', inputMark.poke); field?.addEventListener('blur', inputMark.stop)
    // AppKit draws the mark again when shell output moves the caret, so the
    // wait also restarts on each parsed write while the terminal holds focus.
    const written = terminal.onWriteParsed(() => { if (field && document.activeElement === field) inputMark.poke() })
    const observer = new ResizeObserver(() => { void resize() }); observer.observe(host)
    const themes = new MutationObserver(theme); themes.observe(document.documentElement,{attributes:true,attributeFilter:['data-theme','style']})
    const media = matchMedia('(prefers-color-scheme: dark)'); media.addEventListener('change',theme)
    // The text size step reaches the terminal too, so the super key with = and - sizes every pane.
    const sized = (event) => { terminal.options.fontSize = codeFontSize(event.detail); void resize() }
    window.addEventListener(TYPE_EVENT, sized)
    void start()
    return () => { active = false; clearTimeout(timer); inputMark.stop(); field?.removeEventListener('focus', inputMark.poke); field?.removeEventListener('blur', inputMark.stop); observer.disconnect(); themes.disconnect(); media.removeEventListener('change',theme); window.removeEventListener(TYPE_EVENT, sized); input.dispose(); written.dispose(); terminal.dispose(); if (id) void tauri.invoke('terminal_close',{id}).catch(() => {}) }
  })
</script>
<section class="terminal-workspace" class:hidden aria-label="Terminal">
  <header><span class="path" data-tooltip={cwd || undefined} data-cut={pathCut}><span class="path-text" use:cutStart={cwd}><bdi dir="ltr">{cwd || 'Terminal'}</bdi></span></span>{#if exited || error}<button type="button" disabled={starting} onclick={start}>Restart terminal</button>{/if}</header>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if exited}<p role="status">The shell has exited.</p>{/if}
  <div class="terminal-host" bind:this={host}></div>
</section>
<style>
  .terminal-workspace { flex:1; display:flex; flex-direction:column; min-width:0; min-height:0; background:var(--paper); overflow:hidden; }
  .hidden { display:none; } header { display:flex; align-items:center; gap:12px; padding:8px 12px; border-bottom:1px solid var(--border); background:var(--surface); }
  header { color:var(--muted); font:var(--text-12) var(--font-mono); }
  .path { position:relative; flex:1; min-width:0; }
  /* The path reads left to right inside a right-to-left line, so its start drops behind the ellipsis. */
  .path-text { display:block; overflow:hidden; white-space:nowrap; text-overflow:ellipsis; direction:rtl; text-align:left; }
  .path[data-cut="true"]::after { content:attr(data-tooltip); position:absolute; left:0; top:calc(100% + 4px); z-index:4; width:max-content; max-width:100%; box-sizing:border-box; padding:6px 9px; border:1px solid var(--border); border-radius:var(--radius-control); background:var(--surface); box-shadow:var(--shadow-overlay); color:var(--ink); font:var(--text-12) var(--font-mono); overflow-wrap:anywhere; pointer-events:none; opacity:0; transition:opacity 80ms ease; }
  .path[data-cut="true"]:hover::after { opacity:1; transition-delay:350ms; }
  button { padding:5px 8px; border:1px solid var(--border); border-radius:var(--radius-control); background:transparent; color:var(--ink); font:var(--text-12) var(--font-human); cursor:pointer; }
  p { margin:8px 12px; color:var(--muted); font:var(--text-12) var(--font-mono); }
  /* The prompt keeps the header's 12px inset, clear of the panel edge. */
  .terminal-host { flex:1; min-height:0; padding:8px 12px; overflow:hidden; }
</style>
