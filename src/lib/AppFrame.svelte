<script>
  import { onMount } from 'svelte'
  import Composer from './Composer.svelte'
  import Sidebar from './Sidebar.svelte'
  import { readSidebarCollapsed, writeSidebarCollapsed } from './sidebar-state.js'

  let { subject, onSignOut } = $props()
  let sidebarCollapsed = $state(false)
  let railOpen = $state(false)

  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed
    writeSidebarCollapsed(window.localStorage, sidebarCollapsed)
  }

  function toggleRail() { railOpen = !railOpen }

  onMount(() => {
    sidebarCollapsed = readSidebarCollapsed(window.localStorage)
    const keydown = (event) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'j') {
        event.preventDefault()
        toggleRail()
      }
    }
    window.addEventListener('keydown', keydown)
    return () => window.removeEventListener('keydown', keydown)
  })
</script>

<div class="app-frame">
  <Sidebar collapsed={sidebarCollapsed} {subject} onToggle={toggleSidebar} {onSignOut} />
  <main>
    <header class="titlebar">
      <strong>New thread</strong>
      <button onclick={toggleRail} aria-expanded={railOpen} aria-controls="artifact-rail">Artifacts <kbd>⌘J</kbd></button>
    </header>
    <section class="thread" aria-label="Thread">
      <div class="first-run">Ask anything. Your org's routing decides which model answers.</div>
      <Composer />
    </section>
  </main>
  <aside id="artifact-rail" class:open={railOpen} aria-label="Artifact rail" aria-hidden={!railOpen}>
    <header><strong>Artifacts</strong><button onclick={toggleRail} aria-label="Close artifact rail">×</button></header>
    <p>No artifacts yet</p>
  </aside>
</div>

<style>
  .app-frame { height: 100vh; display: flex; overflow: hidden; }
  main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
  .titlebar { height: 54px; flex: none; display: flex; align-items: center; justify-content: space-between; padding: 0 16px 0 20px; background: var(--surface); border-bottom: 1px solid var(--border); }
  .titlebar strong { font-size: var(--text-15); font-weight: 600; }
  .titlebar button, #artifact-rail button { padding: 6px 8px; color: var(--muted); background: transparent; border: 0; border-radius: var(--radius-control); font: inherit; font-size: var(--text-12); cursor: pointer; }
  .titlebar button:hover, #artifact-rail button:hover { color: var(--ink); background: var(--faint); }
  kbd { margin-left: 5px; font-family: var(--font-mono); font-size: 11px; font-weight: 400; }
  .thread { flex: 1; min-height: 0; display: flex; flex-direction: column; align-items: center; justify-content: flex-end; padding: 40px clamp(24px, 7vw, 88px) 24px; }
  .first-run { flex: 1; display: flex; align-items: flex-end; padding-bottom: 24px; max-width: 36ch; text-align: center; font-size: var(--text-22); font-weight: 500; line-height: var(--leading-heading); }
  #artifact-rail { width: 0; flex: none; overflow: hidden; background: var(--surface); border-left: 0 solid var(--border); transition: width 180ms ease, border-left-width 180ms ease; }
  #artifact-rail.open { width: 300px; border-left-width: 1px; }
  #artifact-rail header { height: 54px; width: 300px; display: flex; align-items: center; justify-content: space-between; padding: 0 14px 0 18px; border-bottom: 1px solid var(--border); }
  #artifact-rail p { width: 300px; padding: 18px; color: var(--muted); font-family: var(--font-mono); font-size: var(--text-12); }
  @media (prefers-reduced-motion: reduce) { #artifact-rail { transition: none; } }
</style>
