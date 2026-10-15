<script>
  // One machine on the Machines page: its kind, version and state, and for a
  // paired machine a menu with Rename and Remove. Remove asks once and names
  // it. An older machine shows that it is updating, since updates run on
  // their own.
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import { kindIcon, kindLabel, statusHint, statusLabel, statusTone, versionLabel } from './runtimes.js'

  let { runtime, appVersion = '', busy = false, onrename, onremove } = $props()
  let mode = $state('view')
  let draft = $state('')
  let menu = $state(false)
  let root = $state()
  const updating = $derived(runtime.updateOffered && runtime.status === 'online')

  $effect(() => {
    if (!menu) return
    const outside = (event) => { if (!root?.contains(event.target)) menu = false }
    window.addEventListener('pointerdown', outside, true)
    return () => window.removeEventListener('pointerdown', outside, true)
  })

  async function rename(event) {
    event.preventDefault()
    if (await onrename(draft.trim())) mode = 'view'
  }

  async function remove() {
    if (await onremove()) mode = 'view'
  }
</script>

{#if mode === 'rename'}
  <form class="runtime-form" aria-label="Rename {runtime.name}" onsubmit={rename}>
    <input type="text" aria-label="Machine name" maxlength="80" bind:value={draft}>
    <Button type="submit" disabled={busy || !draft.trim()}>Save</Button>
    <button type="button" class="quiet" onclick={() => { mode = 'view' }}>Cancel</button>
  </form>
{:else if mode === 'remove'}
  <Row title="Remove {runtime.name}?" description="This computer forgets its access. Its threads stay on it.">
    <Button disabled={busy} onclick={remove}>Remove</Button>
    <button type="button" class="quiet" onclick={() => { mode = 'view' }}>Keep</button>
  </Row>
{:else}
  <Row title={runtime.name} meta={versionLabel(runtime)} label={runtime.name}>
    {#snippet lead()}<span class="runtime-mark" aria-hidden="true"><LucideIcon name={kindIcon(runtime)} size={16} variant="action" /></span>{/snippet}
    {#snippet detail()}
      <p class="runtime-facts">
        {#if !runtime.local}<span>{kindLabel(runtime)}</span>{/if}
        {#if runtime.sshHost}<span class="record">ssh {runtime.sshHost}</span>{/if}
        {#if runtime.sharing === 'shared'}<span>Uses this computer's models</span>{/if}
        {#if updating}
          <span class="runtime-state" data-tone="signal">Updating to v{runtime.updateTarget ?? appVersion}</span>
        {:else}
          <span class="runtime-state" data-tone={statusTone(runtime)}>{statusLabel(runtime)}</span>
        {/if}
      </p>
      {#if statusHint(runtime)}<p class="runtime-hint">{statusHint(runtime)}</p>
      {:else if runtime.sharing === 'no_access'}<p class="runtime-hint">Pair it again with run access to share this computer's models.</p>{/if}
    {/snippet}
    {#if !runtime.local}
      <span class="menu-wrap" bind:this={root}>
        <button type="button" class="quiet more" disabled={busy} aria-label="More for {runtime.name}" aria-haspopup="menu" aria-expanded={menu} onclick={() => { menu = !menu }}><LucideIcon name="ellipsis" size={16} variant="action" /></button>
        {#if menu}
          <div class="row-menu" role="menu" aria-label="{runtime.name} actions">
            <button type="button" role="menuitem" onclick={() => { menu = false; draft = runtime.name; mode = 'rename' }}>Rename</button>
            <button type="button" role="menuitem" onclick={() => { menu = false; mode = 'remove' }}>Remove</button>
          </div>
        {/if}
      </span>
    {/if}
  </Row>
{/if}

<style>
  .runtime-mark { display: grid; place-items: center; width: 32px; height: 32px; box-sizing: border-box; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); }
  .runtime-facts { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; margin: 0; color: var(--muted); font-size: var(--text-13); }
  .record { font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .runtime-state[data-tone="ink"] { color: var(--ink); }
  .runtime-state[data-tone="signal"] { color: var(--ink); }
  .runtime-state[data-tone="caution"] { color: var(--ochre); }
  .runtime-state[data-tone="critical"] { color: var(--oxide); }
  .runtime-hint { margin: 0; color: var(--muted); font-size: var(--text-12); }
  .quiet { background: transparent; }
  .menu-wrap { position: relative; display: inline-flex; }
  .more { display: inline-grid; place-items: center; min-width: 32px; min-height: 32px; }
  .row-menu { position: absolute; z-index: 4; top: calc(100% + 4px); right: 0; display: grid; gap: 2px; min-width: 140px; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); }
  .row-menu button { padding: 6px 10px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .row-menu button:hover, .row-menu button:focus-visible { background: var(--faint); }
  .runtime-form { display: flex; align-items: center; gap: 8px; padding: 10px 0; }
  .runtime-form input { flex: 1; min-width: 0; }
</style>
