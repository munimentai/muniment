<script>
  let { tauri, onmanage } = $props()
  let open = $state(false)
  let settings = $state(null)
  let error = $state('')
  let pending = $state(false)

  $effect(() => {
    if (!open) return
    let disposed = false
    async function refresh() {
      pending = true
      try {
        const next = await tauri.invoke('model_router_settings')
        if (!disposed) { settings = next; error = '' }
      } catch (_) {
        if (!disposed) error = 'Account capacity could not be read.'
      } finally {
        if (!disposed) pending = false
      }
    }
    void refresh()
    const timer = setInterval(refresh, 15000)
    return () => { disposed = true; clearInterval(timer) }
  })
</script>

<details bind:open>
  <summary>Capacity</summary>
  <section aria-label="Account capacity">
    <header><strong>Capacity</strong><button onclick={() => { open = false }} aria-label="Close capacity">×</button></header>
    {#if error}<p role="alert">{error}</p>{:else if !settings}<p role="status">Reading account capacity…</p>{:else}
      {#if !settings.enabled}<p>Account balancing is off.</p>{:else if !settings.running}<p>The account router is not running.</p>{/if}
      {#each settings.accounts ?? [] as account (account.id)}
        <article>
          <strong>{account.label}</strong>
          <p>{account.family} · {account.source === 'key' ? 'Paid API key' : 'Subscription'}</p>
          <p>{!settings.enabled ? 'Excluded while account balancing is off' : !settings.running ? 'Unavailable while the router is stopped' : account.exclusion_reason ?? (account.exclusion_reason === null ? 'Ready for supported models' : 'Readiness unavailable')}</p>
          {#if account.cooldown_until_ms && account.exclusion_reason}<p>Retry after {new Date(account.cooldown_until_ms).toLocaleString()}</p>{/if}
          {#each account.windows ?? [] as window}
            <p>{window.label}{window.scope ? ` · ${window.scope}` : ''}: {window.remaining_percent}% remaining{window.resets_at_ms ? ` · Resets ${new Date(window.resets_at_ms).toLocaleString()}` : ''}</p>
          {:else}<p>Remaining allowance unavailable</p>{/each}
          {#if account.quota_observed_ms}<p>Allowance checked {new Date(account.quota_observed_ms).toLocaleString()}</p>{/if}
        </article>
      {:else}<p>No accounts in the balancing pool. Direct model connections appear in Models &amp; routing.</p>{/each}
    {/if}
    <button onclick={() => { open = false; onmanage() }}>Account details</button>
    {#if pending && settings}<p role="status">Refreshing…</p>{/if}
  </section>
</details>

<style>
  details { position: relative; font-size: var(--text-12); }
  summary { color: var(--muted); cursor: pointer; }
  section { position: absolute; bottom: calc(100% + 12px); left: 0; width: min(320px, 70vw); max-height: 55vh; overflow-y: auto; padding: 12px; background: var(--surface); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-panel); box-shadow: var(--shadow-overlay); }
  header { display: flex; align-items: center; justify-content: space-between; }
  article { padding: 10px 0; border-bottom: 1px solid var(--border); }
  p { margin: 6px 0; color: var(--muted); }
  button { color: var(--ink); background: transparent; border: 1px solid var(--border); border-radius: var(--radius-control); padding: 4px 8px; cursor: pointer; font: inherit; }
</style>
