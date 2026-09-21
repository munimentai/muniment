<script>
  import { panelScroll } from './panel-scroll.js'
  import Toggle from './Toggle.svelte'
  import { tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import AgentAvatar from './AgentAvatar.svelte'
  import MemorySection from './MemorySection.svelte'
  import { newAvatar } from './agent-avatar.js'
  import { exportTemplate, exportGrokSetup, templateSummary } from './agent-templates.js'
  let { agent, tauri, projects = [], run, history = [], earlierConversations = [], onclose, onchange, ondelete, onopen } = $props()
  let editing = $state(null)
  let value = $state('')
  let schedule = $state(null)
  let busy = $state(false)
  let error = $state('')
  let deleting = $state(false)
  let memoryOpen = $state(false)
  let panel
  const days = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday']
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone
  const scheduleText = $derived(!agent.schedule?.enabled ? 'Not scheduled' : `${agent.schedule.cadence === 'weekly' ? days[agent.schedule.weekday] : agent.schedule.cadence === 'weekdays' ? 'Weekdays' : 'Every day'} · ${agent.schedule.time}`)
  async function edit(field) {
    editing = field
    value = agent[field] ?? ''
    schedule = { ...(agent.schedule ?? { enabled: false, cadence: 'daily', time: '09:00', weekday: 0 }) }
    error = ''
    await tick()
    panel?.querySelector('form input, form textarea, form select')?.focus()
  }
  async function endEdit() {
    const field = editing
    editing = null
    await tick()
    panel?.querySelector(`[aria-label="Edit ${field === 'projectId' ? 'project' : field === 'instructions' ? 'description' : field === 'label' ? 'job title' : field}"]`)?.focus()
  }
  async function action(work) {
    busy = true; error = ''
    try { await work() } catch (e) { error = String(e) }
    finally { busy = false }
  }
  async function changeAvatar() {
    await action(async () => {
      const listing = await tauri.invoke('agent_list')
      const latest = listing.agents.find(item => item.id === agent.id)
      if (!latest) throw new Error('This agent no longer exists.')
      await tauri.invoke('agent_save', { agent: { ...latest, avatar: newAvatar() } })
      await onchange()
    })
  }
  async function save() {
    await action(async () => {
      // Read the current definition before applying one field, preserving manual edits.
      const listing = await tauri.invoke('agent_list')
      const latest = listing.agents.find(item => item.id === agent.id)
      if (!latest) throw new Error('This agent no longer exists.')
      const next = { ...latest, [editing]: editing === 'schedule' ? { ...schedule } : editing === 'projectId' ? value || null : value.trim() }
      await tauri.invoke('agent_save', { agent: next })
      await onchange()
      busy = false
      await endEdit()
    })
  }
</script>
<aside data-panel="agent" class="profile" aria-label="Agent profile" bind:this={panel}>
  <header data-panel-header><h2>Agent profile</h2><button data-panel-control class="quiet" aria-label="Collapse agent profile" onclick={onclose}><LucideIcon name="panel-right-close" size={16} /></button></header>
  <div class="body" use:panelScroll>
    <div class="avatar"><AgentAvatar {agent} size={92} /><button class="edit quiet" aria-label="Generate another avatar" disabled={busy} onclick={changeAvatar}><LucideIcon name="refresh-cw" size={14} /></button></div>
    {#each [['name', 'Name'], ['label', 'Job title'], ['instructions', 'Description'], ['projectId', 'Project'], ['schedule', 'Schedule']] as [field, label]}
      <section class="field">
        <div class="field-heading"><h3>{label}</h3>{#if editing !== field}<button class="edit quiet" disabled={busy} aria-label={`Edit ${field === 'projectId' ? 'project' : field === 'instructions' ? 'description' : field === 'label' ? 'job title' : field}`} onclick={() => edit(field)}><LucideIcon name="pencil" size={14} /></button>{/if}</div>
        {#if editing === field}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <form aria-label={`Edit ${label}`} onsubmit={event => { event.preventDefault(); void save() }} onkeydown={event => { if (event.key === 'Escape' && !busy) { event.stopPropagation(); void endEdit() } }}>
            {#if field === 'name' || field === 'label'}<input aria-label={label} bind:value maxlength={field === 'name' ? 100 : 120} required={field === 'name'} disabled={busy} />
            {:else if field === 'instructions'}<textarea aria-label="Description" rows="8" bind:value required disabled={busy}></textarea>
            {:else if field === 'projectId'}<select aria-label="Project" bind:value disabled={busy}><option value="">No project</option>{#each projects as [id, name]}<option value={id}>{name}</option>{/each}</select><small>New work uses this project. Existing files stay in their current folder.</small>
            {:else}
              <label class="check"><Toggle bind:checked={schedule.enabled} disabled={busy} />Run on a schedule</label>
              {#if schedule.enabled}
                <label>Repeat<select bind:value={schedule.cadence} disabled={busy}><option value="daily">Every day</option><option value="weekdays">Weekdays</option><option value="weekly">Every week</option></select></label>
                {#if schedule.cadence === 'weekly'}<label>Day<select bind:value={schedule.weekday} disabled={busy}>{#each days as day, i}<option value={i}>{day}</option>{/each}</select></label>{/if}
                <label>Time<input type="time" bind:value={schedule.time} required disabled={busy} /></label>
                <small>{zone}. Runs while this computer is awake.</small>
              {/if}
            {/if}
            <div class="actions"><button disabled={busy || (['name','instructions'].includes(field) && !value.trim())}>Save</button><button type="button" disabled={busy} onclick={endEdit}>Cancel</button></div>
          </form>
        {:else}
          <p class:name={field === 'name'}>{field === 'projectId' ? projects.find(([id]) => id === agent.projectId)?.[1] || 'No project' : field === 'schedule' ? scheduleText : agent[field] || 'Not set'}</p>
        {/if}
      </section>
    {/each}
    {#if run?.status}<section class="field"><h3>Latest run</h3><p>{run.status}</p>{#if run.error}<p>{run.error}</p>{/if}{#if run.threadId}<button onclick={() => action(() => onopen(run.threadId))}>Open conversation</button>{/if}</section>{/if}
    <details class="template" bind:open={memoryOpen}><summary>Memory</summary>{#if memoryOpen}{#key agent.id}<MemorySection {tauri} agentId={agent.id} />{/key}{/if}</details>
    <details class="template"><summary>Routine history ({history.length})</summary>
      {#each [...history].reverse() as item (item.runId)}<section class="field"><p>{item.lastRun ? new Date(item.lastRun * 1000).toLocaleString() : 'Run'} · {item.status}</p>{#if item.error}<p>{item.error}</p>{/if}<button onclick={() => action(() => onopen(item.threadId))}>Open conversation</button></section>{:else}<p>No routine runs yet.</p>{/each}
    </details>
    {#if earlierConversations.length}<details class="template"><summary>Earlier conversations</summary>{#each earlierConversations as item}<button onclick={() => action(() => onopen(item.threadId))}>{item.title}</button>{/each}</details>{/if}
    {#if agent.template}<details class="template"><summary>Imported template</summary><p>{templateSummary(agent.template)}</p><pre>{JSON.stringify(agent.template, null, 2)}</pre></details>{/if}
    <div class="actions secondary">
      <button disabled={busy || ['queued','running','waiting'].includes(run?.status)} onclick={() => action(async () => { await tauri.invoke('agent_run', { id: agent.id }); await onchange() })}>Run now</button>
      <button disabled={busy} onclick={() => action(() => tauri.invoke('agent_open', { id: agent.id }))}>Open folder</button>
      <button disabled={busy} onclick={() => action(() => tauri.invoke('agent_export_template', { content: exportTemplate(agent), name: agent.name }))}>Export template</button>
      <button disabled={busy} onclick={() => action(() => tauri.invoke('agent_export_template', { content: exportGrokSetup(agent), name: agent.name, format: 'markdown' }))}>Export for Grok Bot</button>
      {#if deleting}<button disabled={busy} onclick={() => action(async () => { await tauri.invoke('agent_delete', { id: agent.id }); ondelete() })}>Confirm delete</button><button disabled={busy} onclick={() => { deleting = false }}>Cancel</button>
      {:else}<button disabled={busy || ['queued','running','waiting'].includes(run?.status)} onclick={() => { deleting = true }}>Delete agent</button>{/if}
    </div>
    {#if error}<p role="alert">{error}</p>{/if}
  </div>
</aside>
<style>
  .profile { grid-area: rail;   display: flex; flex-direction: column; overflow: hidden;     }

  h2 { margin: 0; font-size: var(--text-13); }
  .body { overflow: auto; padding: 8px 16px 24px; }
  .avatar { display: flex; align-items: end; margin-bottom: 8px; }
  .avatar:hover .edit, .avatar:focus-within .edit { opacity: 1; }
  .template { margin-top: 12px; font-size: var(--text-12); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--text-12); }
  .field { padding: 12px 0; border-bottom: 1px solid var(--border); }
  .field-heading { display: flex; justify-content: space-between; align-items: center; min-height: 24px; }
  h3 { margin: 0 0 4px; color: var(--muted); font-size: var(--text-12); font-weight: 500; }
  p { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--text-13); line-height: 1.5; }
  p.name { font-size: var(--text-15); font-weight: 600; }
  .edit { opacity: 0; }
  .field:hover .edit, .field:focus-within .edit { opacity: 1; }
  form, label { display: grid; gap: 8px; font-size: var(--text-12); }
  input, textarea, select { box-sizing: border-box; width: 100%; min-width: 0; padding: 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; }
  textarea { resize: vertical; line-height: 1.5; }
  .check { display: flex; align-items: center; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .secondary { margin-top: 16px; }
  button { font: inherit; font-size: var(--text-12); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 8px; background: transparent; cursor: pointer; }
  button:hover { background: var(--faint); }
  button:disabled { opacity: .5; cursor: default; }
  .quiet { border-color: transparent; padding: 4px; line-height: 0; }
  small { color: var(--muted); line-height: 1.5; }
  @media (hover: none) { .edit { opacity: 1; } }
</style>
