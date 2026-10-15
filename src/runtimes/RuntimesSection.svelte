<script>
  // The Runtimes page of Settings: this computer first, then each paired
  // runtime with its kind, version and state, Switch, Rename and Remove, and
  // Update for an older runtime this device may update. Update all asks every
  // such runtime to install this app's version. Add a runtime pairs a new one.
  import { onMount } from 'svelte'
  import PageHead from '../lib/ui/PageHead.svelte'
  import Group from '../lib/ui/Group.svelte'
  import Button from '../lib/ui/Button.svelte'
  import RuntimeRow from './RuntimeRow.svelte'
  import RuntimeAdd from './RuntimeAdd.svelte'
  import ThisComputer from './ThisComputer.svelte'
  import { errorLine, updatableRuntimes } from './runtimes.js'

  let { tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args), switchDisabled = false } = $props()
  let list = $state(null)
  let error = $state('')
  let notice = $state('')
  let failures = $state([])
  let busy = $state('')
  let checking = $state(false)
  const updatable = $derived(updatableRuntimes(list))

  onMount(() => {
    let alive = true
    let stop
    void load().then(refresh)
    Promise.resolve(listen?.('runtimes-changed', ({ payload }) => { if (payload?.runtimes) list = payload }))
      .then((unlisten) => { if (alive) stop = unlisten; else unlisten?.() })
      .catch(() => {})
    return () => { alive = false; stop?.() }
  })

  async function load() {
    try {
      list = await tauri.invoke('runtime_list')
    } catch (failure) {
      error = errorLine(failure, 'The runtime list could not load.')
    }
  }

  async function refresh() {
    checking = true
    try {
      const next = await tauri.invoke('runtime_refresh')
      if (next?.runtimes) list = next
    } catch (_) {
      // The rows keep the state each runtime answered last.
    } finally {
      checking = false
    }
  }

  function clear() {
    error = ''
    notice = ''
    failures = []
  }

  async function act(id, command, payload) {
    busy = id
    clear()
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

  function report(results) {
    const requested = results.filter((result) => result.ok)
    failures = results.filter((result) => !result.ok)
    if (requested.length === 1) notice = `${requested[0].name}: ${requested[0].message}`
    else if (requested.length) notice = `Update to ${requested[0].version} requested on ${requested.length} runtimes.`
  }

  async function update(runtime) {
    busy = runtime.id
    clear()
    try {
      report([await tauri.invoke('runtime_update', { id: runtime.id })])
    } catch (failure) {
      error = errorLine(failure)
    } finally {
      busy = ''
    }
  }

  async function updateAll() {
    busy = 'all'
    clear()
    try {
      report(await tauri.invoke('runtime_update_all') ?? [])
    } catch (failure) {
      error = errorLine(failure)
    } finally {
      busy = ''
    }
  }
</script>

<section class="settings-runtimes" aria-label="Runtimes" data-settings-controls>
  <PageHead intro="Use this app with Muniment on another computer, server or container. Its keys and files stay there.">
    {#snippet actions()}
      {#if updatable.length}<Button icon="arrow-down" disabled={!!busy} aria-label="Update all" onclick={updateAll}><span class="action-label">Update all</span></Button>{/if}
      <Button icon="refresh-cw" disabled={checking} aria-label="Check runtimes" onclick={refresh}><span class="action-label">{checking ? 'Checking…' : 'Check'}</span></Button>
    {/snippet}
  </PageHead>
  {#if error}<p class="line" role="alert">{error}</p>{/if}
  {#if notice}<p class="line" role="status">{notice}</p>{/if}
  {#each failures as failure (failure.id)}<p class="line" role="alert">{failure.name}: {failure.message}</p>{/each}

  <Group heading="Runtimes" count={list?.runtimes?.length ?? null}>
    {#if list}
      {#each list.runtimes as runtime (runtime.id)}
        <RuntimeRow
          {runtime}
          busy={busy === runtime.id || busy === 'all'}
          {switchDisabled}
          onselect={() => act(runtime.id, 'runtime_select', { id: runtime.id })}
          onrename={(name) => act(runtime.id, 'runtime_rename', { id: runtime.id, name })}
          onremove={() => act(runtime.id, 'runtime_remove', { id: runtime.id })}
          onupdate={() => update(runtime)}
        />
      {/each}
    {:else if !error}
      <p role="status">Loading runtimes…</p>
    {/if}
  </Group>
  {#if list?.runtimes?.length === 1}<p class="line">This app attaches to one runtime at a time. Threads stay on the runtime that holds them.</p>{/if}

  <RuntimeAdd {tauri} deviceName={list?.deviceName ?? ''} onpaired={(next) => { if (next?.runtimes) list = next; clear() }} />

  <ThisComputer {tauri} />
</section>

<style>
  .settings-runtimes { display: grid; gap: var(--page-gap); }
  .action-label { white-space: nowrap; }
  .line { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .line[role="alert"] { color: var(--ink); }
</style>
