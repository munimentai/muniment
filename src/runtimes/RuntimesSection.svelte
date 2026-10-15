<script>
  // The Machines page of Settings: this computer first, then each paired
  // machine with its kind, version and state, and Rename and Remove in its
  // menu. Every machine stays connected, and a thread runs on the machine it
  // started on. An older machine updates on its own. Under the list, one
  // switch shares this computer's model connections with every machine, and
  // for a signed-in account a second keeps them in MuniCloud. Below that,
  // Pair shares this computer and adds another machine.
  import { onMount } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import RuntimeRow from './RuntimeRow.svelte'
  import RuntimeAdd from './RuntimeAdd.svelte'
  import ThisComputer from './ThisComputer.svelte'
  import { errorLine } from './runtimes.js'
  import { featureFlags } from '../feature-flags.js'

  let { tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args) } = $props()
  let list = $state(null)
  let error = $state('')
  let notice = $state('')
  let busy = $state('')
  let signedIn = $state(false)
  const viaCloud = $derived(signedIn && !!list?.shareViaCloud)

  onMount(() => {
    let alive = true
    let stop
    void load().then(refresh)
    if (featureFlags.cloud) {
      Promise.resolve(tauri.invoke('auth_status')).then((status) => { if (alive) signedIn = !!status?.signed_in }).catch(() => {})
    }
    Promise.resolve(listen?.('runtimes-changed', ({ payload }) => { if (payload?.runtimes) list = payload }))
      .then((unlisten) => { if (alive) stop = unlisten; else unlisten?.() })
      .catch(() => {})
    return () => { alive = false; stop?.() }
  })

  async function load() {
    try {
      list = await tauri.invoke('runtime_list')
    } catch (failure) {
      error = errorLine(failure, 'The machine list could not load.')
    }
  }

  async function refresh() {
    try {
      const next = await tauri.invoke('runtime_refresh')
      if (next?.runtimes) list = next
    } catch (_) {
      // The rows keep the state each machine answered last.
    }
  }

  async function act(id, command, payload) {
    busy = id
    error = ''
    notice = ''
    try {
      const next = await tauri.invoke(command, payload)
      if (next?.runtimes) list = next
      if (next?.notice) notice = next.notice
      return true
    } catch (failure) {
      error = errorLine(failure)
      return false
    } finally {
      busy = ''
    }
  }
</script>

<section class="settings-runtimes" aria-label="Machines" data-settings-controls>
  {#if error}<p class="line" role="alert">{error}</p>{/if}
  {#if notice}<p class="line" role="status">{notice}</p>{/if}

  <Group heading="Machines" count={list?.runtimes?.length ?? null}>
    {#if list}
      {#each list.runtimes as runtime (runtime.id)}
        <RuntimeRow
          {runtime}
          appVersion={list.appVersion}
          busy={busy === runtime.id}
          onrename={(name) => act(runtime.id, 'runtime_rename', { id: runtime.id, name })}
          onremove={() => act(runtime.id, 'runtime_remove', { id: runtime.id })}
        />
      {/each}
    {:else if !error}
      <p role="status">Loading machines…</p>
    {/if}
  </Group>

  <Group label="Model sharing">
    <Row title="Share model connections" description="Paired machines use the providers, sign-ins and keys connected on this computer.">
      <Toggle checked={list?.shareModels ?? false} label="Share model connections" disabled={!list || busy === 'share'} onchange={(next) => act('share', 'runtime_share_models', { share: next })} />
    </Row>
    {#if signedIn}
      <Row title="Keep them in MuniCloud" description="MuniCloud holds them encrypted and renews sign-ins. A free account shares with up to five machines.">
        <Toggle checked={viaCloud} label="Keep them in MuniCloud" disabled={!list?.shareModels || busy === 'share'} onchange={(next) => act('share', 'runtime_share_models', { share: true, cloud: next })} />
      </Row>
    {/if}
  </Group>
  {#if list?.shareError}<p class="line" role="alert">{list.shareError}</p>{/if}
  <div class="share-notice" role="note">
    <span class="share-notice-icon"><LucideIcon name="info" size={16} variant="action" /></span>
    {#if viaCloud}
      <p>MuniCloud renews shared sign-ins, so paired machines keep them while this computer is off. Each machine counts toward your account's pairings.</p>
    {:else}
      <p>Shared subscriptions depend on this computer. When it sleeps or goes offline, a paired machine can lose them within 30 minutes. Shared API keys keep working.</p>
    {/if}
  </div>

  <ThisComputer {tauri}>
    {#snippet add()}
      <RuntimeAdd {tauri} deviceName={list?.deviceName ?? ''} onpaired={(next) => { if (next?.runtimes) list = next; error = ''; notice = '' }} />
    {/snippet}
  </ThisComputer>
</section>

<style>
  .settings-runtimes { display: grid; gap: var(--page-gap); }
  .line { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .line[role="alert"] { color: var(--ink); }
  .share-notice { display: flex; align-items: flex-start; gap: 10px; margin-top: calc(var(--page-gap) * -0.5); padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--faint); }
  .share-notice-icon { display: inline-flex; flex: none; padding-top: 1px; color: var(--muted); }
  .share-notice p { margin: 0; color: var(--ink); font-size: var(--text-13); }
</style>
