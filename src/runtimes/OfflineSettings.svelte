<script>
  // What this computer keeps available offline: threads, attachments,
  // artifacts, projects and skills. Core keeps the thread window and the
  // attachments with `fleet.offline_set`, and pins and the artifact choice
  // with `fleet.pin_set`. A runtime outside a fleet refuses, and the group
  // stays hidden.
  import { onMount, tick } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import SegmentedField from '../lib/ui/SegmentedField.svelte'
  import { agentRequest } from '../agents/agent-runtime.js'
  import { LOCAL_RUNTIME_ID } from './runtimes.js'
  import { ARTIFACT_CHOICES, ATTACHMENT_CHOICES, PROJECT_CHOICES, THREAD_CHOICES, offlineScope } from './fleet.js'

  let { tauri } = $props()
  const id = $props.id()
  let status = $state(null)
  let pins = $state(null)
  let projects = $state({})
  let error = $state('')
  let busy = $state(false)
  let choosing = $state(false)
  const scope = $derived(offlineScope(status, pins))
  const shown = $derived(!!status && status.state !== 'alone')
  const ask = (operation, body = {}) => agentRequest(tauri, LOCAL_RUNTIME_ID, operation, body)

  // The page reads its own settings first, then this group.
  onMount(() => {
    void tick().then(() => {
      void ask('fleet.sync_status').then((answer) => { status = answer }).catch(() => {})
      void ask('fleet.pins').then((answer) => { pins = answer }).catch(() => {})
      void Promise.resolve(tauri.invoke('project_list')).then((catalog) => { projects = catalog?.projects ?? {} }).catch(() => {})
    })
  })

  async function save(action) {
    busy = true
    error = ''
    try { await action() } catch (failure) { error = String(failure?.message ?? failure) } finally { busy = false }
  }

  const setThreads = (days) => save(async () => {
    const answer = await ask('fleet.offline_set', { thread_days: days })
    status = { ...status, offline: answer.offline }
  })
  const setAttachments = (attachments) => save(async () => {
    const answer = await ask('fleet.offline_set', { attachments })
    status = { ...status, offline: answer.offline }
  })
  const setArtifacts = (artifacts) => save(async () => { pins = await ask('fleet.pin_set', { offline: { artifacts } }) })
  // TODO(core): `fleet.pin_set` pins artifacts and extensions; a project
  // pin keeps a read-only snapshot of its files once core takes the class.
  const pinProject = (key, pinned) => save(async () => { pins = await ask('fleet.pin_set', { pins: [{ class: 'project', key, pinned }] }) })

  function setProjects(value) {
    if (value === 'chosen') { choosing = true; return }
    choosing = false
    for (const key of scope.pinnedProjects) void pinProject(key, false)
  }
</script>

{#if shown}
  <Group heading="Available offline" description="What this computer keeps when the machine that owns it is away.">
    <Row title="Threads" description="Older threads load from their machine on open.">
      <SegmentedField label="Threads" hideLabel value={scope.threads} options={THREAD_CHOICES} onchange={(value) => { if (!busy) void setThreads(value) }} />
    </Row>
    <Row title="Attachments" description="Files in threads download when you open them, or all at once.">
      <SegmentedField label="Attachments" hideLabel value={scope.attachments} options={ATTACHMENT_CHOICES} onchange={(value) => { if (!busy) void setAttachments(value) }} />
    </Row>
    <Row title="Artifacts" description="Pin an artifact to keep its files here.">
      <SegmentedField label="Artifacts" hideLabel value={scope.artifacts} options={ARTIFACT_CHOICES} onchange={(value) => { if (!busy) void setArtifacts(value) }} />
    </Row>
    <Row title="Projects" description="Project lists always sync. Chosen projects also keep their files.">
      <SegmentedField label="Projects" hideLabel value={choosing ? 'chosen' : scope.projects} options={PROJECT_CHOICES} onchange={setProjects} />
    </Row>
    {#if choosing || scope.projects === 'chosen'}
      <div class="project-picks" role="group" aria-labelledby="{id}-projects">
        <p id="{id}-projects" class="picks-label">Keep these projects offline</p>
        {#each Object.entries(projects) as [key, name] (key)}
          <label class="pick"><input type="checkbox" checked={scope.pinnedProjects.includes(key)} disabled={busy} onchange={(event) => pinProject(key, event.currentTarget.checked)} /><span>{name}</span></label>
        {:else}
          <p class="picks-label">No projects yet.</p>
        {/each}
      </div>
    {/if}
    <Row title="Skills and plugins" description="Every skill and plugin enabled here stays available.">
      <span class="fixed">All</span>
    </Row>
  </Group>
  {#if error}<p class="offline-error" role="alert">{error}</p>{/if}
{/if}

<style>
  .project-picks { display: grid; gap: 6px; padding: 10px 0; }
  .picks-label { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .pick { display: flex; align-items: center; gap: 8px; min-height: 24px; color: var(--ink); font-size: var(--text-13); }
  .fixed { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .offline-error { margin: 0; color: var(--ink); font-size: var(--text-13); }
</style>
