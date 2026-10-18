<script>
  // The Machines page of Settings: this computer first, then each paired
  // machine with its kind, version, state and update state, and Update,
  // Rename and Remove in its menu. Every machine stays connected, and a
  // thread runs on the machine it started on. An older machine updates on its
  // own. Update all asks every machine that is behind and answers, names the
  // ones it skips, and updates this computer last. Under the list, one
  // switch has core share this computer's model connections with the fleet
  // through `fleet.credentials`, and for a signed-in account a second keeps
  // them in MuniCloud. The swarm
  // follows: linked machines and their links. Below that, Pair shares this
  // computer and adds another machine, and Log lists the fleet's audit log.
  import { onMount } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import PageHead from '../lib/ui/PageHead.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ConfirmDialog from '../lib/ConfirmDialog.svelte'
  import RuntimeRow from './RuntimeRow.svelte'
  import RuntimeAdd from './RuntimeAdd.svelte'
  import ThisComputer from './ThisComputer.svelte'
  import AuditLog from './AuditLog.svelte'
  import SwarmSection from '../agents/SwarmSection.svelte'
  import { errorLine, skipLabel } from './runtimes.js'
  import { agentRequest } from '../agents/agent-runtime.js'
  import { interruptionCount, interruptionMessage } from '../lib/app-update.js'
  import { featureFlags } from '../feature-flags.js'

  let { tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args) } = $props()
  let list = $state(null)
  let error = $state('')
  let notice = $state('')
  let busy = $state('')
  let signedIn = $state(false)
  let skipped = $state([])
  let failures = $state([])
  let localUpdate = $state(null)
  let shareModels = $state(null)
  const viaCloud = $derived(signedIn && !!list?.shareViaCloud)

  onMount(() => {
    let alive = true
    let stop
    void load().then(refresh)
    void readSharing()
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

  // Core keeps the sharing switch: `fleet.credentials` answers it and sets it.
  async function readSharing() {
    try {
      shareModels = !!(await agentRequest(tauri, 'local', 'fleet.credentials', {}))?.share_models
    } catch (_) {
      shareModels = false
    }
  }

  // Turning sharing off takes the connections back from MuniCloud too.
  async function setSharing(next) {
    busy = 'share'
    error = ''
    notice = ''
    try {
      const answer = await agentRequest(tauri, 'local', 'fleet.credentials', { share_models: next })
      shareModels = !!answer?.share_models
    } catch (failure) {
      error = errorLine(failure)
      return
    } finally {
      busy = ''
    }
    if (!next && list?.shareViaCloud) await act('share', 'runtime_share_models', { cloud: false })
  }

  function named(names) {
    return names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`
  }

  // Asks one machine to install the version this app brings it to.
  async function updateOne(runtime) {
    busy = runtime.id
    error = ''
    notice = ''
    try {
      const result = await tauri.invoke('runtime_update', { id: runtime.id })
      if (result?.ok) notice = result.message
      else error = errorLine(result?.message)
    } catch (failure) {
      error = errorLine(failure)
    } finally {
      busy = ''
    }
  }

  // Asks every paired machine that is behind and answers, then updates this
  // computer last, since its install restarts the app.
  async function updateAll() {
    busy = 'all'
    error = ''
    notice = ''
    skipped = []
    failures = []
    try {
      const report = await tauri.invoke('runtime_update_all')
      const results = report?.results ?? []
      const asked = results.filter((result) => result.ok).map((result) => result.name)
      failures = results.filter((result) => !result.ok).map((result) => `${result.name}: ${errorLine(result.message)}`)
      const skips = [...(report?.skipped ?? [])]
      let version = null
      try { version = await tauri.invoke('app_update_prepare') } catch { /* This computer keeps its version. */ }
      if (!version) skips.push({ id: 'local', name: 'This computer', reason: 'current' })
      skipped = skips
      notice = asked.length ? `Update requested on ${named(asked)}.` : ''
      if (version) await updateThisComputer(version)
    } catch (failure) {
      error = errorLine(failure)
    } finally {
      busy = ''
    }
  }

  async function updateThisComputer(version) {
    const count = await interruptionCount(tauri)
    if (count > 0) localUpdate = { version, count }
    else await installThisComputer()
  }

  async function installThisComputer() {
    try {
      await tauri.invoke('app_update_install')
    } catch (failure) {
      error = errorLine(failure, 'The update could not be installed. Try again.')
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

  {#each failures as failure (failure)}<p class="line" role="alert">{failure}</p>{/each}
  {#if skipped.length}<p class="line" role="status">Skipped: {skipped.map(skipLabel).join(', ')}.</p>{/if}

  <PageHead intro="This computer and the machines paired with it.">
    {#snippet actions()}
      <Button icon="arrow-down" disabled={!list || !!busy} onclick={updateAll}>Update all</Button>
    {/snippet}
  </PageHead>
  <Group label="Machine list">
    {#if list}
      {#each list.runtimes as runtime (runtime.id)}
        <RuntimeRow
          {runtime}
          hostname={runtime.local ? list.deviceName ?? '' : ''}
          busy={busy === runtime.id || busy === 'all'}
          onrename={(name) => act(runtime.id, 'runtime_rename', { id: runtime.id, name })}
          onremove={() => act(runtime.id, 'runtime_remove', { id: runtime.id })}
          onupdate={() => updateOne(runtime)}
        />
      {/each}
    {:else if !error}
      <p role="status">Loading machines…</p>
    {/if}
  </Group>

  {#if list}<SwarmSection {tauri} {list} {listen} />{/if}

  <Group label="Model sharing">
    <Row title="Share model connections" info="Paired machines use the providers, sign-ins and keys connected on this computer.">
      <Toggle checked={shareModels ?? false} label="Share model connections" disabled={shareModels === null || busy === 'share'} onchange={setSharing} />
    </Row>
    {#if signedIn}
      <Row title="Keep them in MuniCloud" info="MuniCloud holds them encrypted and renews sign-ins. A free account shares with up to five machines.">
        <Toggle checked={viaCloud} label="Keep them in MuniCloud" disabled={!list || !shareModels || busy === 'share'} onchange={(next) => act('share', 'runtime_share_models', { cloud: next })} />
      </Row>
    {/if}
  </Group>
  {#if list?.shareError}<p class="line" role="alert">{list.shareError}</p>{/if}
  <div class="share-notice" role="note">
    <span class="share-notice-icon"><LucideIcon name="info" size={16} variant="action" /></span>
    {#if viaCloud}
      <p>MuniCloud renews shared sign-ins while this computer is off. Each machine counts toward your pairings.</p>
    {:else}
      <p>Shared subscriptions depend on this computer. Paired machines can lose them 30 minutes after it sleeps. API keys keep working.</p>
    {/if}
  </div>

  {#if localUpdate}
    <ConfirmDialog title="Install update {localUpdate.version}?" cancelLabel="Not now" confirmLabel="Install and restart" onDecision={async (approve) => { localUpdate = null; if (approve) await installThisComputer() }}>
      <p>{interruptionMessage(localUpdate.count)}</p>
    </ConfirmDialog>
  {/if}

  <ThisComputer {tauri}>
    {#snippet add()}
      <RuntimeAdd {tauri} deviceName={list?.deviceName ?? ''} onpaired={(next) => { if (next?.runtimes) list = next; error = ''; notice = next?.notice ?? '' }} />
    {/snippet}
  </ThisComputer>

  <AuditLog {tauri} />
</section>

<style>
  .settings-runtimes { display: grid; gap: var(--page-gap); }
  .line { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .line[role="alert"] { color: var(--ink); }
  .share-notice { display: flex; align-items: flex-start; gap: 10px; margin-top: calc(var(--page-gap) * -0.5); padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--faint); }
  .share-notice-icon { display: inline-flex; flex: none; padding-top: 1px; color: var(--muted); }
  .share-notice p { margin: 0; color: var(--ink); font-size: var(--text-13); }
</style>
