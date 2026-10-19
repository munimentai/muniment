<script>
  // Settings → Permissions: the runtime's permission settings from core's
  // `permission.settings`, saved one change at a time through
  // `permission.settings_save`. Core validates each change and refuses with
  // its reason. A paired machine keeps its own settings. While Settings
  // changes all machines, each save names that scope, and a setting core
  // keeps on each machine waits for one machine to be picked.
  import { onMount } from 'svelte'
  import Toggle from './Toggle.svelte'
  import PageHead from './ui/PageHead.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import ChoiceField from './ui/ChoiceField.svelte'
  import Button from './ui/Button.svelte'
  import LucideIcon from './LucideIcon.svelte'
  import PermissionRules from '../agents/PermissionRules.svelte'
  import { agentRequest } from '../agents/agent-runtime.js'
  import { loadModes } from '../agents/permission-modes.js'

  let { tauri, runtime = null, machineName = 'This computer', all = false, onsaved = () => {} } = $props()
  let settings = $state(null)
  let modes = $state(null)
  let minutes = $state(240)
  let busy = $state(false)
  let error = $state('')
  let status = $state('')

  async function load() {
    const [read, listing] = await Promise.all([agentRequest(tauri, runtime, 'permission.settings', {}), loadModes(tauri, runtime)])
    settings = read
    modes = listing
    minutes = Math.round(read.unattended_timeout_seconds / 60)
  }
  onMount(() => { load().catch((failure) => { error = `Permission settings did not load. ${String(failure?.message ?? failure)}` }) })

  async function save(change) {
    busy = true
    error = ''
    status = ''
    try {
      settings = await agentRequest(tauri, runtime, 'permission.settings_save', all ? { ...change, scope: 'all' } : change)
      minutes = Math.round(settings.unattended_timeout_seconds / 60)
      modes = await loadModes(tauri, runtime)
      status = 'Saved.'
      onsaved()
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }
  // A setting each machine keeps for itself, while every machine is in scope.
  const perMachine = (field) => all && (settings?.machine_local ?? []).includes(field)
  const choices = $derived((modes?.modes ?? []).filter((mode) => mode.available !== false).map((mode) => ({ value: mode.id, label: mode.title })))
</script>

<section class="permission-settings" aria-label="Permission settings">
  <PageHead intro={`How agents and threads on ${machineName} ask before they act.`} />
  {#if settings}
    <Group heading="Default modes">
      <Row title="New threads">
        <ChoiceField label="Default thread mode" hideLabel value={settings.default_thread_mode} options={choices} disabled={busy} onchange={(value) => save({ default_thread_mode: value })} />
      </Row>
      <Row title="Agents" description="Unless the agent picks its own">
        <ChoiceField label="Default agent mode" hideLabel value={settings.default_agent_mode} options={choices} disabled={busy} onchange={(value) => save({ default_agent_mode: value })} />
      </Row>
    </Group>
    <Group heading="Bypass permissions">
      <Row title="Allow bypass" description={perMachine('bypass_enabled') ? 'Each machine keeps its own. Pick one machine to change it.' : ''} info="Off, new threads and agents start in Ask before changes, and a default set to Bypass permissions moves to Ask before changes.">
        {#snippet detail()}
          <p class="warning"><LucideIcon name="triangle-alert" variant="action" size={14} />Everything runs except deny and ask-first rules, protected paths and critical commands.</p>
        {/snippet}
        <Toggle checked={settings.bypass_enabled} label="Allow bypass permissions" disabled={busy || perMachine('bypass_enabled')} onchange={(enabled) => save({ bypass_enabled: enabled })} />
      </Row>
    </Group>
    <Group heading="Unattended runs">
      <Row title="Wait for an answer" description="Then the action is denied and the run stops" info="An agent run nobody watches waits this long for an answer.">
        <form class="wait" onsubmit={(event) => { event.preventDefault(); void save({ unattended_timeout_seconds: Math.round(minutes * 60) }) }}>
          <input type="number" aria-label="Unattended wait (minutes)" min="1" max="10080" step="1" bind:value={minutes} required disabled={busy} />
          <span class="unit">min</span>
          <Button type="submit" disabled={busy || Math.round(minutes * 60) === settings.unattended_timeout_seconds}>Save wait</Button>
        </form>
      </Row>
    </Group>
    <Group heading="Rules for every thread" info={`Exact requests every thread and agent on ${machineName} denies, asks first or runs.`}>
      <div class="rules">
        <PermissionRules rules={settings} {busy} onsave={(list, requests) => save({ [list]: requests })} />
      </div>
    </Group>
  {:else if !error}
    <p role="status">Loading permission settings…</p>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if status}<p role="status">{status}</p>{/if}
</section>

<style>
  .permission-settings { display: grid; gap: var(--page-gap); }
  p { margin: 0; font-size: var(--text-13); color: var(--muted); }
  .warning { display: flex; align-items: flex-start; gap: 6px; margin-top: 4px; color: var(--ink); font-size: var(--text-12); line-height: 1.45; }
  .warning > :global(svg) { flex: none; margin-top: 1px; color: var(--ochre); }
  .wait { display: flex; align-items: center; gap: 6px; }
  .unit { color: var(--muted); font: var(--text-12) var(--font-mono); }
  input[type="number"] { width: 96px; height: 32px; box-sizing: border-box; padding: 0 10px; color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-13) var(--font-mono); }
  input:focus { outline: none; border-color: var(--muted); }
  .rules { display: grid; gap: 6px; padding: 12px 0; }
  .error { color: var(--oxide); }
</style>
