<script>
  // The swarm on the Machines page: every member with its link state, Link
  // machines for two machines this device manages, and Unlink and Leave.
  // Linking asks one machine for an invitation and hands it to the other,
  // so no link passes through the user. Core keeps and dials the links.
  import { onMount } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import StatusChip from '../lib/ui/StatusChip.svelte'
  import { onlineTone } from '../lib/status-tone.js'
  import { agentRequest } from './agent-runtime.js'
  import { directLinks, linkLabel, linkMachines, listSwarm, memberMachine, windowRuntime } from './swarm.js'
  import { LOCAL_RUNTIME_ID, kindIcon } from '../runtimes/runtimes.js'

  let { tauri, list = null, listen = (...args) => window.__TAURI__?.event?.listen(...args) } = $props()
  let swarm = $state(null)
  let error = $state('')
  let notice = $state('')
  let busy = $state('')
  let from = $state(LOCAL_RUNTIME_ID)
  let to = $state('')

  // Machines this device may link: this computer and each paired machine it
  // administers that answers.
  const managed = $derived((list?.runtimes ?? []).filter((runtime) => runtime.local || (runtime.admin && runtime.status === 'online')))
  const members = $derived(swarm?.members ?? [])
  const linked = $derived(Object.values(swarm?.views ?? {}).some((view) => directLinks(view).length > 0) || members.some((member) => !windowRuntime(swarm, member.runtime_id)))

  onMount(() => {
    let alive = true
    let stop
    void load()
    Promise.resolve(listen?.('swarm-changed', ({ payload }) => { if (alive && payload?.change?.kind === 'members') void load() }))
      .then((unlisten) => { if (alive) stop = unlisten; else unlisten?.() })
      .catch(() => {})
    return () => { alive = false; stop?.() }
  })

  $effect(() => { if ((!to || to === from) && managed.length > 1) to = managed.find((runtime) => runtime.id !== from)?.id ?? '' })

  async function load() {
    try { swarm = await listSwarm(tauri, list) } catch (failure) { error = String(failure?.message ?? failure) }
  }

  async function act(key, work, done) {
    busy = key
    error = ''
    notice = ''
    try { await work(); await load(); notice = done } catch (failure) { error = String(failure?.message ?? failure) } finally { busy = '' }
  }

  const nameOf = (id) => managed.find((runtime) => runtime.id === id)?.name ?? 'Machine'

  // The machines that keep a direct link to `coreId`, with their own view.
  function linksTo(coreId) {
    return Object.entries(swarm?.views ?? {}).flatMap(([runtime, view]) => directLinks(view).filter((member) => member.runtime_id === coreId).map((member) => ({ runtime, link: member.link })))
  }

  function unlink(coreId, name) {
    const holders = linksTo(coreId)
    return act(`unlink:${coreId}`, () => Promise.all(holders.map(({ runtime }) => agentRequest(tauri, runtime, 'swarm.leave', { runtime_id: coreId }))), `Unlinked ${name}.`)
  }

  function leave(runtime) {
    const view = swarm?.views?.[runtime]
    return act(`leave:${runtime}`, () => Promise.all(directLinks(view).map((member) => agentRequest(tauri, runtime, 'swarm.leave', { runtime_id: member.runtime_id }))), `${nameOf(runtime)} left the swarm.`)
  }
</script>

<Group heading="Swarm" count={members.length || null} description="Linked machines share their agents, agent threads, groups and approvals.">
  {#if error}<p class="line" role="alert">{error}</p>{/if}
  {#if notice}<p class="line" role="status">{notice}</p>{/if}
  {#if !swarm}
    <p role="status">Loading the swarm…</p>
  {:else if !linked}
    <p>No machines are linked yet.</p>
  {/if}
  {#each members as member (member.runtime_id)}
    {@const reached = windowRuntime(swarm, member.runtime_id)}
    {@const links = linksTo(member.runtime_id)}
    {@const machine = memberMachine(swarm, member.runtime_id, list)}
    <Row title={machine.name} label={`Swarm member ${machine.name}`}>
      {#snippet lead()}<LucideIcon name={kindIcon(machine)} />{/snippet}
      {#snippet detail()}
        <span class="member-state"><StatusChip tone={onlineTone(member.online)}>{member.online ? 'Online' : 'Offline'}</StatusChip></span>
        {#if reached}
          <p class="state">{directLinks(swarm.views[reached]).length ? `Links to ${directLinks(swarm.views[reached]).map((entry) => memberMachine(swarm, entry.runtime_id, list).name).join(', ')}` : 'Not linked'}</p>
        {:else}
          {#each links as entry (entry.runtime)}<p class="state">{linkLabel(entry.link)}</p>{:else}<p class="state">{linkLabel(null)}</p>{/each}
        {/if}
      {/snippet}
      {#if reached && managed.some((runtime) => runtime.id === reached) && directLinks(swarm.views[reached]).length}
        <Button variant="outline" disabled={!!busy} onclick={() => leave(reached)}>Leave swarm</Button>
      {:else if !reached && links.length}
        <Button variant="outline" disabled={!!busy} onclick={() => unlink(member.runtime_id, machine.name)}>Unlink</Button>
      {/if}
    </Row>
  {/each}
  {#if managed.length > 1}
    <Row title="Link machines" description="Both machines must accept connections. This device asks one for an invitation and gives it to the other." stacked>
      <div class="link-form">
        <label>From<select bind:value={from} disabled={!!busy}>{#each managed as runtime (runtime.id)}<option value={runtime.id}>{runtime.name}</option>{/each}</select></label>
        <label>To<select bind:value={to} disabled={!!busy}>{#each managed.filter((runtime) => runtime.id !== from) as runtime (runtime.id)}<option value={runtime.id}>{runtime.name}</option>{/each}</select></label>
        <Button icon="link" variant="primary" disabled={!!busy || !to || to === from} onclick={() => act('link', () => linkMachines(tauri, from, to), `Linked ${nameOf(from)} and ${nameOf(to)}.`)}>{busy === 'link' ? 'Linking…' : 'Link machines'}</Button>
      </div>
    </Row>
  {:else}
    <p>Pair another machine with admin access to link it here.</p>
  {/if}
</Group>

<style>
  .line { margin: 0; padding: 12px 0; color: var(--muted); font-size: var(--text-13); }
  .line[role="alert"] { color: var(--oxide); }
  .member-state { display: flex; }
  .state { margin: 2px 0 0; color: var(--muted); font: var(--text-12) var(--font-human); overflow-wrap: anywhere; }
  .link-form { display: flex; flex-wrap: wrap; align-items: end; gap: 8px; }
  label { display: grid; gap: 4px; color: var(--muted); font-size: var(--text-12); }
  select { min-height: 32px; padding: 5px 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; font-size: var(--text-13); }
</style>
