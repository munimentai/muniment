<script>
  // The Machines page of Settings: this computer first, then each paired
  // machine with its kind, version and state, and Rename and Remove in its
  // menu. Every machine stays connected, and a thread runs on the machine it
  // started on. An older machine updates on its own. Below the list, Pair
  // shares this computer and adds another machine.
  import { onMount } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import RuntimeRow from './RuntimeRow.svelte'
  import RuntimeAdd from './RuntimeAdd.svelte'
  import ThisComputer from './ThisComputer.svelte'
  import { errorLine } from './runtimes.js'

  let { tauri, listen = (...args) => window.__TAURI__?.event?.listen(...args) } = $props()
  let list = $state(null)
  let error = $state('')
  let notice = $state('')
  let busy = $state('')

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
</style>
