<script>
  // The update control: it appears once a verified update is downloaded.
  // An install stops the replies this computer runs, so it first names how
  // many and asks. They resume after the update.
  import { onMount } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import ConfirmDialog from './ConfirmDialog.svelte'
  import { interruptionCount, interruptionMessage } from './app-update.js'
  let { tauri, busy = false, listen = (...args) => window.__TAURI__?.event?.listen(...args) } = $props()
  let version = $state(null), installing = $state(false), error = $state(''), stopping = $state(0)
  onMount(() => {
    let disposed = false, checking = false
    async function check() {
      if (checking || version || installing) return
      checking = true
      try { const next = await tauri.invoke('app_update_prepare'); if (!disposed && next) version = next }
      catch { /* Offline checks leave the workspace unchanged. Retry on the next interval. */ }
      finally { checking = false }
    }
    void check()
    const timer = setInterval(check, 60 * 60 * 1000)
    // An update a paired client requested downloads in the app, which then shows it here.
    let stop
    Promise.resolve(listen?.('app-update-ready', ({ payload }) => { if (!disposed && typeof payload === 'string') version = payload }))
      .then((unlisten) => { if (disposed) unlisten?.(); else stop = unlisten })
      .catch(() => {})
    return () => { disposed = true; clearInterval(timer); stop?.() }
  })
  async function install() {
    if (busy || installing) return
    installing = true
    error = ''
    const count = await interruptionCount(tauri)
    if (count > 0) { stopping = count; return }
    await proceed()
  }
  async function proceed() {
    try { await tauri.invoke('app_update_install') }
    catch (failure) { error = typeof failure === 'string' ? failure : 'The update could not be installed. Try again.'; installing = false }
  }
  async function decide(approve) {
    stopping = 0
    if (approve) await proceed()
    else installing = false
  }
</script>
{#if version}
  <div class="update">
    <button type="button" class:installing disabled={busy || installing} aria-label={installing ? 'Installing update' : `Install update ${version} and restart`} aria-describedby={busy ? 'update-busy-reason' : undefined} onclick={install}>
      <span>{installing ? 'Installing' : 'Update'}</span><LucideIcon name="arrow-down" variant="action" size={14} />
    </button>
    {#if busy}<span id="update-busy-reason" hidden>Finish the current action before updating.</span>{/if}
    {#if error}<p role="alert">{error}</p>{/if}
  </div>
{/if}
{#if stopping}
  <ConfirmDialog title="Install update {version}?" cancelLabel="Not now" confirmLabel="Install and restart" onDecision={decide}>
    <p>{interruptionMessage(stopping)}</p>
  </ConfirmDialog>
{/if}
<style>
  .update { position: relative; display: flex; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 0; height: 24px; min-width: 24px; padding: 0 5px; border: 0; border-radius: var(--radius-pill); background: var(--signal-soft); color: var(--signal); font: var(--text-12) var(--font-mono); cursor: pointer; }
  span { max-width: 0; opacity: 0; overflow: hidden; white-space: nowrap; transition: max-width 150ms ease, opacity 150ms ease, margin 150ms ease; }
  button:hover span, button:focus-visible span, .installing span { max-width: 90px; opacity: 1; margin: 0 4px 0 2px; }
  button:disabled { cursor: default; opacity: .6; }
  p { position: absolute; right: 0; top: 100%; width: 260px; padding: 12px; background: var(--surface); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-panel); z-index: 20; }
  @media (prefers-reduced-motion: reduce) { span { transition: none; } }
</style>
