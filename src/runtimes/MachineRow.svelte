<script>
  // One row of the Machines list: the machine's shape, name and version, a
  // kind chip, one state chip and Limited for a device without admin access.
  // A row this app can change has a menu with Update, Rename and Remove.
  // Remove asks once and names the machine. `details` holds this computer's
  // own settings behind a disclosure. The update chip shows the state the
  // machine reports, never an update the machine has not confirmed. A phone
  // that must pair again says so, and its menu offers Pair again.
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import StatusChip from '../lib/ui/StatusChip.svelte'
  import { machineStatusTone, onlineTone, updateStatusTone } from '../lib/status-tone.js'
  import { canRequestUpdate, seenLabel, statusHint, statusLabel, updateLabel } from './runtimes.js'
  import { rowKind } from './machines.js'

  // `hostname` names this computer to the OS, under its one name in the app.
  let { row, hostname = '', busy = false, removable = false, open = $bindable(false), onrename, onremove, onupdate, onpairagain = null, details } = $props()
  const id = $props.id()
  let mode = $state('view')
  let draft = $state('')
  let menu = $state(false)
  let root = $state()
  const runtime = $derived(row.runtime)
  const kind = $derived(rowKind(row))
  const update = $derived(runtime?.status === 'online' ? updateLabel(runtime) : null)
  const UPDATE_STATES = ['queued', 'downloading', 'draining', 'installing', 'restarting', 'failed']
  const updateTone = $derived(updateStatusTone(UPDATE_STATES.includes(runtime?.updateStatus?.state) ? runtime.updateStatus.state : runtime?.updateRequested ? 'requested' : 'available'))
  const canUpdate = $derived(!!runtime && canRequestUpdate(runtime) && !!onupdate)
  const canRename = $derived(!!runtime && !runtime.local)
  const canPairAgain = $derived(!!row.pairAgain && !!onpairagain)
  const hasMenu = $derived(canUpdate || canRename || canPairAgain || removable)
  // A machine that answers shows Online. One that does not, and a device
  // that reports no state, shows when it was last seen.
  const seen = $derived.by(() => {
    if (row.online || (row.online === false && !row.seenAt)) return ''
    const label = seenLabel(row.seenAt)
    return label === 'Never seen' ? label : `Seen ${label === 'Now' ? 'now' : label}`
  })

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
  <form class="runtime-form" aria-label="Rename {row.name}" onsubmit={rename}>
    <input type="text" aria-label="Machine name" maxlength="80" bind:value={draft}>
    <Button type="submit" disabled={busy || !draft.trim()}>Save</Button>
    <button type="button" class="quiet" onclick={() => { mode = 'view' }}>Cancel</button>
  </form>
{:else if mode === 'remove'}
  <Row title="Remove {row.name}?" description="It leaves your machines and loses access to this computer. Its threads stay on it.">
    <Button disabled={busy} onclick={remove}>Remove</Button>
    <button type="button" class="quiet" onclick={() => { mode = 'view' }}>Keep</button>
  </Row>
{:else}
  <Row title={row.name} meta={row.version} label={row.name}>
    {#snippet lead()}<span class="runtime-mark" aria-hidden="true"><LucideIcon name={row.icon} size={16} variant="action" /></span>{/snippet}
    {#snippet detail()}
      <p class="runtime-facts">
        {#if row.local && hostname}<span class="record">{hostname}</span>{/if}
        {#if kind}<span class="kind">{kind}</span>{/if}
        {#if runtime?.sshHost}<span class="record">ssh {runtime.sshHost}</span>{/if}
        {#if update}
          <span class="runtime-state" data-tooltip={update.tooltip || undefined} tabindex={update.tooltip ? 0 : undefined}><StatusChip tone={updateTone}>{update.text}</StatusChip></span>
        {:else if runtime}
          <span class="runtime-state"><StatusChip tone={machineStatusTone(runtime.status)}>{statusLabel(runtime)}</StatusChip></span>
        {:else if row.online !== null}
          <span class="runtime-state"><StatusChip tone={onlineTone(row.online)}>{row.online ? 'Online' : 'Offline'}</StatusChip></span>
        {/if}
        {#if row.pairAgain}<span class="runtime-state"><StatusChip tone="warning">Pair again</StatusChip></span>{/if}
        {#if row.limited}<span class="runtime-state" data-tooltip="Reads and runs threads. It cannot update or add machines." tabindex="0"><StatusChip>Limited</StatusChip></span>{/if}
        {#if seen}<span class="seen">{seen}</span>{/if}
      </p>
      {#if runtime && statusHint(runtime)}<p class="runtime-hint">{statusHint(runtime)}</p>{/if}
      {#if row.pairAgain}<p class="runtime-hint">This phone cannot open sessions from this computer until it pairs again.</p>{/if}
    {/snippet}
    {#if details}
      <button type="button" class="quiet more" aria-expanded={open} aria-controls="{id}-details" aria-label="Settings for {row.name}" onclick={() => { open = !open }}><LucideIcon name={open ? 'chevron-up' : 'chevron-down'} size={16} variant="action" /></button>
    {/if}
    {#if hasMenu}
      <span class="menu-wrap" bind:this={root}>
        <button type="button" class="quiet more" disabled={busy} aria-label="More for {row.name}" aria-haspopup="menu" aria-expanded={menu} onclick={() => { menu = !menu }}><LucideIcon name="ellipsis" size={16} variant="action" /></button>
        {#if menu}
          <div class="row-menu" role="menu" aria-label="{row.name} actions">
            {#if canUpdate}<button type="button" role="menuitem" onclick={() => { menu = false; onupdate() }}>Update</button>{/if}
            {#if canRename}<button type="button" role="menuitem" onclick={() => { menu = false; draft = runtime.name; mode = 'rename' }}>Rename</button>{/if}
            {#if canPairAgain}<button type="button" role="menuitem" onclick={() => { menu = false; onpairagain() }}>Pair again</button>{/if}
            {#if removable}<button type="button" role="menuitem" class="critical" onclick={() => { menu = false; mode = 'remove' }}>Remove</button>{/if}
          </div>
        {/if}
      </span>
    {/if}
  </Row>
  {#if details && open}<div class="details" id="{id}-details">{@render details()}</div>{/if}
{/if}

<style>
  .runtime-mark { display: grid; place-items: center; width: 32px; height: 32px; box-sizing: border-box; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); }
  .runtime-facts { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; margin: 0; color: var(--muted); font-size: var(--text-13); }
  .record { font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .kind { padding: 0 6px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font-size: var(--text-12); line-height: 16px; }
  .seen { font-size: var(--text-12); }
  .runtime-state { display: inline-flex; }
  .runtime-hint { margin: 0; color: var(--muted); font-size: var(--text-12); }
  .runtime-state[data-tooltip] { position: relative; cursor: default; }
  .runtime-state[data-tooltip]::after { content: attr(data-tooltip); position: absolute; z-index: 4; left: 0; bottom: calc(100% + 6px); width: max-content; max-width: 280px; padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font-size: var(--text-12); pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  .runtime-state[data-tooltip]:hover::after, .runtime-state[data-tooltip]:focus-visible::after { opacity: 1; transition-delay: 350ms; }
  .quiet { background: transparent; }
  .menu-wrap { position: relative; display: inline-flex; }
  .more { display: inline-grid; place-items: center; min-width: 32px; min-height: 32px; }
  .row-menu { position: absolute; z-index: 4; top: calc(100% + 4px); right: 0; display: grid; gap: 2px; min-width: 140px; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); }
  .row-menu button { padding: 6px 10px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .row-menu button.critical { color: var(--danger); }
  .row-menu button:hover, .row-menu button:focus-visible { background: var(--faint); }
  .runtime-form { display: flex; align-items: center; gap: 8px; padding: 10px 0; }
  .runtime-form input { flex: 1; min-width: 0; }
  .details { display: grid; gap: var(--page-gap); padding: 4px 0 16px 48px; }
</style>
