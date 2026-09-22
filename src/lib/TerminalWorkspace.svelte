<script>
  import OverflowText from './OverflowText.svelte'
  import { onMount, tick } from 'svelte'
  import { Terminal } from '@xterm/xterm'
  import { FitAddon } from '@xterm/addon-fit'
  import '@xterm/xterm/css/xterm.css'
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
    } catch (e) { if (active) error = String(e) }
    finally { starting = false }
  }
  $effect(() => { if (!hidden) void tick().then(() => { theme(); void resize() }) })
  onMount(() => {
    terminal = new Terminal({ fontFamily: 'Commit Mono, monospace', fontSize: 13, cursorBlink: true, scrollback: 5000, allowProposedApi: false })
    fit = new FitAddon(); terminal.loadAddon(fit); terminal.open(host); theme()
    const input = terminal.onData(data => { if (!id || exited) return; const session = id; inputQueue = inputQueue.then(() => tauri.invoke('terminal_write', { id: session, data })).catch(e => { if (active) error = String(e) }) })
    const observer = new ResizeObserver(() => { void resize() }); observer.observe(host)
    const themes = new MutationObserver(theme); themes.observe(document.documentElement,{attributes:true,attributeFilter:['data-theme','style']})
    const media = matchMedia('(prefers-color-scheme: dark)'); media.addEventListener('change',theme)
    void start()
    return () => { active = false; clearTimeout(timer); observer.disconnect(); themes.disconnect(); media.removeEventListener('change',theme); input.dispose(); terminal.dispose(); if (id) void tauri.invoke('terminal_close',{id}).catch(() => {}) }
  })
</script>
<section class="terminal-workspace" class:hidden aria-label="Terminal">
  <header><OverflowText text={cwd || 'Terminal'} />{#if exited || error}<button type="button" disabled={starting} onclick={start}>Restart terminal</button>{/if}</header>
  {#if error}<p role="alert">{error}</p>{/if}
  {#if exited}<p role="status">The shell has exited.</p>{/if}
  <div class="terminal-host" bind:this={host}></div>
</section>
<style>
  .terminal-workspace { flex:1; display:flex; flex-direction:column; min-width:0; min-height:0; background:var(--paper); overflow:hidden; }
  .hidden { display:none; } header { display:flex; align-items:center; gap:12px; padding:8px 12px; border-bottom:1px solid var(--border); background:var(--surface); }
  header { color:var(--muted); font:var(--text-12) var(--font-mono); }
  button { padding:5px 8px; border:1px solid var(--border); border-radius:var(--radius-control); background:transparent; color:var(--ink); font:var(--text-12) var(--font-human); cursor:pointer; }
  p { margin:8px 12px; color:var(--muted); font:var(--text-12) var(--font-mono); }
  .terminal-host { flex:1; min-height:0; padding:8px; overflow:hidden; }
</style>
