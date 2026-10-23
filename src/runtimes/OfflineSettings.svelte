<script>
  // What this computer keeps available offline: threads, attachments,
  // artifacts, projects and skills. Core keeps the thread window and the
  // attachments with `fleet.offline_set`, and the artifact choice with
  // `fleet.pin_set`. Projects and skills are fixed rows: a project's details
  // and membership sync and its files stay on the machine that owns it, and
  // every skill syncs. The group hides outside a fleet.
  import { onMount, tick } from 'svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import SegmentedField from '../lib/ui/SegmentedField.svelte'
  import { agentRequest } from '../agents/agent-runtime.js'
  import { LOCAL_RUNTIME_ID } from './runtimes.js'
  import { ARTIFACT_CHOICES, ATTACHMENT_CHOICES, THREAD_CHOICES, offlineScope } from './fleet.js'

  let { tauri } = $props()
  let status = $state(null)
  let pins = $state(null)
  let error = $state('')
  let busy = $state(false)
  const scope = $derived(offlineScope(status, pins))
  const shown = $derived(!!status && status.state !== 'alone')
  const ask = (operation, body = {}) => agentRequest(tauri, LOCAL_RUNTIME_ID, operation, body)

  // The page reads its own settings first, then this group.
  onMount(() => {
    void tick().then(() => {
      void ask('fleet.sync_status').then((answer) => { status = answer }).catch(() => {})
      void ask('fleet.pins').then((answer) => { pins = answer }).catch(() => {})
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
</script>

{#if shown}
  <Group heading="Available offline" info="What this computer keeps when the machine that owns it is away.">
    <Row title="Threads" info="Older threads load from their machine on open.">
      <SegmentedField label="Threads" hideLabel value={scope.threads} options={THREAD_CHOICES} onchange={(value) => { if (!busy) void setThreads(value) }} />
    </Row>
    <Row title="Attachments" info="Files in threads download when you open them, or all at once.">
      <SegmentedField label="Attachments" hideLabel value={scope.attachments} options={ATTACHMENT_CHOICES} onchange={(value) => { if (!busy) void setAttachments(value) }} />
    </Row>
    <Row title="Artifacts" info="Pin an artifact to keep its files here.">
      <SegmentedField label="Artifacts" hideLabel value={scope.artifacts} options={ARTIFACT_CHOICES} onchange={(value) => { if (!busy) void setArtifacts(value) }} />
    </Row>
    <Row title="Projects" info="Details only. Files stay on the machine that owns the project.">
      <span class="fixed">Details</span>
    </Row>
    <Row title="Skills and plugins" info="Every skill and plugin enabled here stays available.">
      <span class="fixed">All</span>
    </Row>
  </Group>
  {#if error}<p class="offline-error" role="alert">{error}</p>{/if}
{/if}

<style>
  .fixed { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .offline-error { margin: 0; color: var(--ink); font-size: var(--text-13); }
</style>
