<script>
  // Settings → Machines: one list of every machine and device, this computer
  // first, from the machines this app reaches, the other members of this
  // computer's fleet and the devices that use this computer. Each row shows
  // its kind, version and one state chip, and its menu holds Update, Rename
  // and Remove. Remove drops every link to the machine and its access in one
  // step. Add machine shows this computer's code and takes the other's, and
  // core links a machine with admin access into the fleet as it pairs. Update
  // all shows while an update waits: it asks every machine that is behind and
  // answers, names the ones it skips, and updates this computer last. Under
  // the list, one switch has core share this computer's model connections
  // through `fleet.credentials`. This computer's row opens its network,
  // offline and log settings. With MuniCloud on, the account's devices and
  // the phone paired through the relay are rows as well, and Add machine
  // pairs a phone.
  import { onMount } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import PageHead from '../lib/ui/PageHead.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import ConfirmDialog from '../lib/ConfirmDialog.svelte'
  import MachineRow from './MachineRow.svelte'
  import AddMachine from './AddMachine.svelte'
  import ThisComputer from './ThisComputer.svelte'
  import { errorText, skipLabel, updatableRuntimes } from './runtimes.js'
  import FailureNote from '../lib/FailureNote.svelte'
  import { linkHolders, machineRows, removable } from './machines.js'
  import { agentRequest } from '../agents/agent-runtime.js'
  import { listSwarm } from '../agents/swarm.js'
  import { interruptionCount, interruptionMessage } from '../lib/app-update.js'
  import PhonePairing from '../lib/PhonePairing.svelte'
  import { featureFlags } from '../feature-flags.js'

  // `focus` names the part a link or search result opens: `add` opens Add
  // machine and `this-computer` opens this computer's settings.
  let { tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args), focus = null } = $props()
  let list = $state(null)
  let swarm = $state(null)
  let devices = $state([])
  let cloudDevices = $state([])
  let phone = $state(null)
  let phonePairing = $state()
  let error = $state('')
  let notice = $state('')
  // Why the machine just added did not join this computer's machines. It
  // stays until the next add, since sharing and syncing wait on the link.
  let linkFailure = $state('')
  let busy = $state('')
  let skipped = $state([])
  let failures = $state([])
  let localUpdate = $state(null)
  let localVersion = $state(null)
  let shareModels = $state(null)
  let shareFailure = $state('')
  // svelte-ignore state_referenced_locally
  let adding = $state(focus === 'add')
  // svelte-ignore state_referenced_locally
  let computerOpen = $state(focus === 'this-computer')
  const rows = $derived(list ? machineRows({ list, swarm, devices, cloudDevices, phone }) : [])
  const updateWaits = $derived(!!localVersion || updatableRuntimes(list).length > 0)

  onMount(() => {
    let alive = true
    const stops = []
    void load().then(refresh)
    void readSharing()
    void readDevices()
    void readCloudDevices()
    // Update all shows once this computer has a verified update waiting.
    Promise.resolve(tauri.invoke('app_update_prepare')).then((version) => { if (alive && version) localVersion = version }).catch(() => {})
    for (const [event, handle] of [
      ['runtimes-changed', (payload) => { if (payload?.runtimes) { list = payload; void readSwarm() } }],
      ['swarm-changed', (payload) => { if (payload?.change?.kind === 'members') void readSwarm() }],
      ['app-update-ready', (payload) => { if (typeof payload === 'string') localVersion = payload }],
    ]) {
      Promise.resolve(listen?.(event, ({ payload }) => handle(payload)))
        .then((unlisten) => { if (alive) stops.push(unlisten); else unlisten?.() })
        .catch(() => {})
    }
    return () => { alive = false; for (const stop of stops) stop?.() }
  })

  async function load() {
    try {
      list = await tauri.invoke('runtime_list')
      void readSwarm()
    } catch (failure) {
      error = errorText(failure, 'The machine list could not load.')
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

  // A machine without a fleet refuses, and the list shows the machines this
  // app reaches.
  async function readSwarm() {
    try { swarm = await listSwarm(tauri, list) } catch (_) { swarm = null }
  }

  async function readDevices() {
    try {
      const listed = await tauri.invoke('local_devices')
      devices = Array.isArray(listed?.devices) ? listed.devices : []
    } catch (_) {
      devices = []
    }
  }

  // The devices on the MuniCloud account, while signed in.
  async function readCloudDevices() {
    if (!featureFlags.cloud) return
    try {
      const listed = await tauri.invoke('auth_devices')
      cloudDevices = Array.isArray(listed) ? listed : Array.isArray(listed?.devices) ? listed.devices : []
    } catch (_) {
      cloudDevices = []
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

  // Turning sharing off takes the connections back from MuniCloud too. The
  // switch shows what core answers: a refused change moves it back, and the
  // reason shows under it.
  async function setSharing(next) {
    const before = shareModels
    shareModels = next
    busy = 'share'
    error = ''
    notice = ''
    shareFailure = ''
    try {
      const answer = await agentRequest(tauri, 'local', 'fleet.credentials', { share_models: next })
      shareModels = !!answer?.share_models
    } catch (failure) {
      shareModels = before
      shareFailure = errorText(failure)
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
      else error = errorText(result?.message)
    } catch (failure) {
      error = errorText(failure)
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
      failures = results.filter((result) => !result.ok).map((result) => `${result.name}: ${errorText(result.message)}`)
      const skips = [...(report?.skipped ?? [])]
      let version = null
      try { version = await tauri.invoke('app_update_prepare') } catch { /* This computer keeps its version. */ }
      if (!version) skips.push({ id: 'local', name: 'This computer', reason: 'current' })
      skipped = skips
      notice = asked.length ? `Update requested on ${named(asked)}.` : ''
      if (version) await updateThisComputer(version)
    } catch (failure) {
      error = errorText(failure)
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
      error = errorText(failure, 'The update could not be installed. Try again.')
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
      error = errorText(failure)
      return false
    } finally {
      busy = ''
    }
  }

  // Removes a machine in one step: every machine this app reaches drops its
  // link to it, this computer revokes the access it holds here, and this app
  // forgets its own access there.
  async function remove(row) {
    busy = row.key
    error = ''
    notice = ''
    try {
      if (row.coreId) {
        const left = await Promise.allSettled(linkHolders(swarm, row.coreId).map((runtime) => agentRequest(tauri, runtime, 'swarm.leave', { runtime_id: row.coreId })))
        const refused = left.find((result) => result.status === 'rejected')
        if (refused) throw refused.reason
      }
      if (row.deviceId) await tauri.invoke('local_device_revoke', { deviceId: row.deviceId })
      if (row.pairId) await phonePairing.revokePair(row.pairId)
      if (row.runtime) {
        const next = await tauri.invoke('runtime_remove', { id: row.runtime.id })
        if (next?.runtimes) list = next
        if (next?.notice) notice = next.notice
      }
      if (!notice) notice = `Removed ${row.name}.`
      return true
    } catch (failure) {
      error = errorText(failure)
      return false
    } finally {
      busy = ''
      await Promise.all([readSwarm(), readDevices()])
    }
  }

  // A phone that must pair again loses its pairing and the new code shows.
  async function pairPhoneAgain(row) {
    busy = row.key
    error = ''
    try {
      await phonePairing.revokePair(row.pairId)
      phone = null
      await phonePairing.requestChallenge()
    } catch (failure) {
      error = errorText(failure)
    } finally {
      busy = ''
    }
  }

  function onadded(next) {
    if (next?.runtimes) list = next
    linkFailure = next?.notice ?? ''
    error = ''
    void readSwarm()
    void readDevices()
  }
</script>

<section class="settings-runtimes" aria-label="Machines" data-settings-controls>
  <PageHead intro="This computer and every machine and device that works with it. Machines you add share agents, threads and approvals.">
    {#snippet actions()}
      {#if updateWaits}<Button icon="arrow-down" disabled={!list || !!busy} onclick={updateAll}>Update all</Button>{/if}
      <Button icon="plus" variant="primary" aria-expanded={adding} onclick={() => { adding = !adding }}>Add machine</Button>
    {/snippet}
  </PageHead>

  {#if error}<FailureNote failure={error} />{/if}
  {#if notice}<p class="line" role="status">{notice}</p>{/if}
  {#if linkFailure}<FailureNote failure={linkFailure} />{/if}
  {#each failures as failure (failure)}<FailureNote {failure} />{/each}
  {#if skipped.length}<p class="line" role="status">Skipped: {skipped.map(skipLabel).join(', ')}.</p>{/if}

  {#if adding}
    <AddMachine {tauri} deviceName={list?.deviceName ?? ''} {onadded} onnetwork={() => { computerOpen = true }} onclose={() => { adding = false }} onphone={featureFlags.cloud && phonePairing && !phone ? () => phonePairing.requestChallenge() : null} />
  {/if}
  {#if featureFlags.cloud}<PhonePairing {tauri} compact bind:this={phonePairing} onpair={(next) => { phone = next }} />{/if}

  <Group label="Machine list">
    {#if list}
      {#each rows as row (row.key)}
        {#if row.local}
          <MachineRow {row} hostname={list.deviceName ?? ''} busy={busy === 'all'} bind:open={computerOpen}>
            {#snippet details()}<ThisComputer {tauri} />{/snippet}
          </MachineRow>
        {:else}
          <MachineRow
            {row}
            busy={busy === row.key || busy === row.runtime?.id || busy === 'all'}
            removable={removable(row, swarm)}
            onrename={(name) => act(row.runtime.id, 'runtime_rename', { id: row.runtime.id, name })}
            onremove={() => remove(row)}
            onupdate={row.runtime ? () => updateOne(row.runtime) : null}
            onpairagain={row.pairAgain ? () => pairPhoneAgain(row) : null}
          />
        {/if}
      {/each}
    {:else if !error}
      <p role="status">Loading machines…</p>
    {/if}
  </Group>

  <Group label="Model sharing">
    <Row title="Share model connections" info={list?.shareViaCloud
      ? 'Your other machines use the providers, sign-ins and keys connected on this computer. MuniCloud renews shared sign-ins while this computer is off.'
      : 'Your other machines use the providers, sign-ins and keys connected on this computer. Shared subscriptions depend on this computer, and other machines can lose them 30 minutes after it sleeps. API keys keep working.'}>
      <Toggle checked={shareModels ?? false} label="Share model connections" disabled={shareModels === null || busy === 'share'} onchange={setSharing} />
    </Row>
  </Group>
  {#if shareFailure}<FailureNote failure={shareFailure} />{/if}
  {#if list?.shareError}<FailureNote failure={list.shareError} />{/if}

  {#if localUpdate}
    <ConfirmDialog title="Install update {localUpdate.version}?" cancelLabel="Not now" confirmLabel="Install and restart" onDecision={async (approve) => { localUpdate = null; if (approve) await installThisComputer() }}>
      <p>{interruptionMessage(localUpdate.count)}</p>
    </ConfirmDialog>
  {/if}
</section>

<style>
  .settings-runtimes { display: grid; gap: var(--page-gap); }
  .line { margin: 0; color: var(--muted); font-size: var(--text-13); }
</style>
