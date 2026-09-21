<script>
  import { panelScroll } from './panel-scroll.js'
  import LucideIcon from './LucideIcon.svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import AllowanceMeter from './AllowanceMeter.svelte'
  let { tauri, onmanage, onclose } = $props()
  let settings = $state(null)
  let error = $state('')
  $effect(() => {
    let disposed = false
    async function refresh() {
      try {
        const next = await tauri.invoke('model_router_settings')
        if (!disposed) { settings = next; error = '' }
      } catch (_) { if (!disposed) error = 'Account capacity could not be read.' }
    }
    void refresh()
    const timer = setInterval(refresh, 15000)
    return () => { disposed = true; clearInterval(timer) }
  })
</script>
<div data-panel="capacity" use:panelScroll data-panel-variant="overlay" data-composer-panel class="capacity" role="dialog" aria-label="Account capacity">
  <header><strong>Capacity</strong><button onclick={onclose} aria-label="Close capacity"><LucideIcon name="x" /></button></header>
  {#if error}<p role="alert">{error}</p>{:else if !settings}<p role="status">Reading account capacity…</p>{:else}
    {#if !settings.enabled}<p>Account balancing is off.</p>{:else if !settings.running}<p>The account router is not running.</p>{/if}
    <div class="accounts">
    {#each (settings.accounts ?? []).filter(account => account.source === 'account') as account (account.id)}
      <article>
        <h3><ProviderLogo provider={account.family} /><span>{account.label}</span></h3>
        {#if account.exclusion_reason}<p>{account.exclusion_reason}</p>{/if}
        {#each account.windows ?? [] as window}
          <div class="allowance">
            <div class="amount"><span>{window.label}{window.scope ? ` · ${window.scope}` : ''}</span><strong>{Math.round(window.remaining_percent)}%</strong></div>
            <AllowanceMeter remaining={window.remaining_percent} limited={window.limit_reached} />
          </div>
        {:else}<p>{account.source === 'key' ? 'Usage billed by API' : 'Allowance unavailable'}</p>{/each}
      </article>
    {:else}<p>No accounts connected.</p>{/each}
    </div>
  {/if}
  <footer><button onclick={() => { onclose(); onmanage() }}>Account details</button></footer>
</div>
<style>
  .capacity { width: 100%; box-sizing: border-box; max-height: 60vh; overflow-y: auto; padding: var(--panel-control-inset); font: var(--text-12) var(--font-human); }
  header, .amount, h3 { display: flex; align-items: center; gap: 8px; }
  header, .amount { justify-content: space-between; }
  .accounts { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(180px, 100%), 1fr)); gap: 12px; margin: 12px 0; }
  article { min-width: 0; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-control); }
  h3 { margin: 0 0 12px; font: inherit; font-weight: 600; }
  h3 span { overflow-wrap: anywhere; min-width: 0; }
  .allowance { margin-top: 10px; }
  .amount { margin-bottom: 5px; color: var(--muted); }
  .amount strong { color: var(--ink); }
  p { margin: 6px 0; color: var(--muted); }
  button { color: var(--ink); background: transparent; border: 1px solid var(--border); border-radius: var(--radius-control); padding: 4px 8px; cursor: pointer; font: inherit; }
</style>
