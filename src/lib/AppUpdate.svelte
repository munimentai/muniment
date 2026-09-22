<script>
  import { onMount } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  let { tauri, busy = false } = $props()
  let version = $state(null), installing = $state(false), error = $state('')
  onMount(() => {
    let disposed = false, checking = false
    async function check() {
      if (checking || version || installing) return
      checking = true
      try { const next = await tauri.invoke('app_update_prepare'); if (!disposed) version = next }
      catch { /* Offline checks leave the workspace unchanged. Retry on the next interval. */ }
      finally { checking = false }
    }
    void check()
    const timer = setInterval(check, 60 * 60 * 1000)
    return () => { disposed = true; clearInterval(timer) }
  })
  async function install() {
    if (busy || installing) return
    installing = true
    error = ''
    try { await tauri.invoke('app_update_install') }
    catch (failure) { error = typeof failure === 'string' ? failure : 'The update could not be installed. Try again.'; installing = false }
  }
</script>
{#if version}
  <div class="update">
    <button type="button" class:installing disabled={busy || installing} aria-label={installing ? 'Installing update' : `Install update ${version} and restart`} title={busy ? 'Finish the current action before updating.' : `Install ${version} and restart`} onclick={install}>
      <span>{installing ? 'Installing' : 'Update'}</span><LucideIcon name="arrow-down" variant="action" size={16} />
    </button>
    {#if error}<p role="alert">{error}</p>{/if}
  </div>
{/if}
<style>
  .update { position: relative; display: flex; }
  button { display: inline-flex; align-items: center; justify-content: center; gap: 0; height: 28px; min-width: 28px; padding: 6px; border: 0; border-radius: var(--radius-pill); background: var(--signal-soft); color: var(--signal); font: var(--text-12) var(--font-mono); cursor: pointer; }
  span { max-width: 0; opacity: 0; overflow: hidden; white-space: nowrap; transition: max-width 150ms ease, opacity 150ms ease, margin 150ms ease; }
  button:hover span, button:focus-visible span, .installing span { max-width: 90px; opacity: 1; margin-right: 6px; }
  button:disabled { cursor: default; opacity: .6; }
  p { position: absolute; right: 0; top: 100%; width: 260px; padding: 12px; background: var(--surface); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-panel); z-index: 20; }
  @media (prefers-reduced-motion: reduce) { span { transition: none; } }
</style>
