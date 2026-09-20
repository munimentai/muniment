<script>
  import Toggle from './Toggle.svelte'
  import { onMount, tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import AgentAvatar from './AgentAvatar.svelte'
  import { newAvatar } from './agent-avatar.js'
  import { importTemplate, exportTemplate, exportGrokSetup, publicTemplateUrl, importGrokPage, templateSummary } from './agent-templates.js'
  let { tauri, projects = [], onclose, onstart, onopen, onselect, onchange = () => {}, initialId = null, createNew = false } = $props()
  const empty = () => ({ id: '', name: '', label: '', instructions: '', avatar: newAvatar(), template: null, projectId: null, schedule: { enabled: false, cadence: 'daily', time: '09:00', weekday: 0 } })
  let listing = $state({ agents: [], state: { runs: {}, threads: {} } })
  let draft = $state(empty())
  let busy = $state(false)
  let status = $state('')
  let deleting = $state(false)
  let catalog = $state(false)
  let loading = $state(true)
  let linkOpen = $state(false)
  let shareLink = $state('')
  let panel
  const run = $derived(listing.state.runs[draft.id])
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone
  async function refresh() {
    try { listing = await tauri.invoke('agent_list'); onchange(listing) }
    catch (e) { status = String(e) }
  }
  onMount(() => {
    void refresh().then(() => {
      const selected = listing.agents.find(agent => agent.id === initialId)
      if (selected) edit(selected)
      else catalog = !createNew && listing.agents.length > 0
      loading = false
    })
    const timer = setInterval(refresh, 10000)
    return () => clearInterval(timer)
  })
  function add() { draft = empty(); catalog = false; deleting = false; status = ''; void tick().then(() => panel?.querySelector('input')?.focus()) }
  function scheduleLabel(agent) {
    const s = agent.schedule
    if (!s?.enabled) return ''
    const day = ['Monday','Tuesday','Wednesday','Thursday','Friday','Saturday','Sunday'][s.weekday]
    return `${s.cadence === 'weekly' ? day : s.cadence === 'weekdays' ? 'Weekdays' : 'Every day'} · ${s.time}`
  }
  async function loadTemplate(event) {
    const file = event.target.files?.[0]
    if (!file) return
    try {
      if (file.size > 131072) throw new Error('Choose a template under 128 KB.')
      edit(importTemplate(await file.text()))
      status = 'Template loaded. Review the description and choose a project before saving.'
    } catch (e) { status = String(e) }
    event.target.value = ''
  }
  async function loadLink(event) {
    event.preventDefault(); busy = true; status = ''
    try {
      const page = await tauri.invoke('agent_import_link', { url: publicTemplateUrl(shareLink) })
      edit(importGrokPage(page)); linkOpen = false
      status = 'Grok profile loaded. Review it before saving.'
    } catch (e) { status = String(e) }
    finally { busy = false }
  }
  async function shareTemplate(grok = false) {
    busy = true
    try {
      const saved = await tauri.invoke('agent_export_template', { content: grok ? exportGrokSetup(draft) : exportTemplate(draft), name: draft.name, format: grok ? 'markdown' : 'json' })
      if (saved) status = grok ? 'Setup file exported. Attach it in Grok Bot and ask it to create the Bot.' : 'Template exported with a paused schedule.'
    } catch (e) { status = String(e) }
    finally { busy = false }
  }
  function edit(agent) { catalog = false; draft = { ...agent, schedule: { ...(agent.schedule ?? empty().schedule) } }; deleting = false; status = '' }
  async function save() {
    const saved = await tauri.invoke('agent_save', { agent: { ...draft, projectId: draft.projectId || null, schedule: { ...draft.schedule } } })
    edit(saved)
    await refresh()
    return saved
  }
  async function action(kind) {
    busy = true
    try {
      if (kind === 'delete') {
        await tauri.invoke('agent_delete', { id: draft.id }); draft = empty(); deleting = false; await refresh(); catalog = listing.agents.length > 0; status = 'Agent deleted. Its thread history and files remain.'
      } else {
        const saved = await save()
        if (kind === 'run') { await tauri.invoke('agent_run', { id: saved.id }); await refresh(); status = 'Run queued. The background service starts it when the current reply ends.' }
        else if (kind === 'thread') { await onstart(saved); onclose() }
        else status = 'Agent saved.'
      }
    } catch (e) { status = String(e) }
    finally { busy = false }
  }
  async function openFolder() {
    try { await tauri.invoke('agent_open', { id: draft.id }) } catch(e) { status = String(e) }
  }
  function formatted(time) { return time ? new Date(time * 1000).toLocaleString() : '' }
</script>
<section class="panel" role="region" aria-label="Agents" bind:this={panel}>
  <header>
    <div class="heading"><h2 id="agents-title">{catalog ? 'Agents' : draft.id ? draft.name : 'New agent'}</h2>
      {#if !catalog && listing.agents.length}<button class="quiet" onclick={() => { catalog = true; status = '' }}>All agents</button>{/if}
    </div>
    <div class="actions">
      <button disabled={busy} onclick={() => { linkOpen = !linkOpen }}>Import Grok link</button>
      <label class="import">Import template<input type="file" accept=".json,.md,.txt,application/json,text/markdown" onchange={loadTemplate} disabled={busy} /></label>
      <button class="quiet" aria-label="Close agents" onclick={onclose}><LucideIcon name="x" /></button>
    </div>
  </header>
  {#if linkOpen}<form class="link-form" onsubmit={loadLink}><label>Grok share link<input type="url" required placeholder="https://x.ai/bot/…" bind:value={shareLink} disabled={busy} /></label><button disabled={busy || !shareLink.trim()}>Load template</button></form>{/if}
  {#if loading}<p class="loading">Loading agents…</p>
  {:else if catalog}
    <div class="catalog" aria-label="Agent catalog">
      <button class="card new-card" onclick={add}><LucideIcon name="plus" size={28} /><span>New agent</span></button>
      {#each listing.agents as agent (agent.id)}
        <button class="card" onclick={async () => { try { await onselect(agent) } catch (e) { status = String(e) } }}>
          <AgentAvatar {agent} size={56} />
          <strong>{agent.name}</strong>
          {#if agent.label}<span>{agent.label}</span>{/if}
          <span>{projects.find(([id]) => id === agent.projectId)?.[1] || 'No project'}</span>
          {#if scheduleLabel(agent)}<small>{scheduleLabel(agent)}</small>{/if}
          <small>{listing.state.runs[agent.id]?.status || 'Ready'}</small>
        </button>
      {/each}
    </div>
    {#if status}<p class="loading" role="status">{status}</p>{/if}
  {:else}
      <div class="body">
        <p>Give your agent a name and instructions.</p>
        <form onsubmit={event => { event.preventDefault(); void action('save') }}>
          <div class="draft-avatar"><AgentAvatar agent={draft} size={80} /><button type="button" aria-label="Generate another avatar" disabled={busy} onclick={() => { draft.avatar = newAvatar() }}>Change avatar</button></div>
          <label>Name<input bind:value={draft.name} maxlength="100" required disabled={busy} placeholder="Research assistant" /></label>
          <label>Job title<input bind:value={draft.label} maxlength="120" disabled={busy} placeholder="Research assistant" /></label>
          <label>Description<textarea rows="7" bind:value={draft.instructions} required disabled={busy} placeholder="Describe its role, sources, rules, and expected result."></textarea></label>

          {#if draft.template}<details><summary>Imported template details</summary><p>{templateSummary(draft.template)}</p><pre>{JSON.stringify(draft.template, null, 2)}</pre></details>{/if}
          <label>Project<select bind:value={draft.projectId} disabled={busy}><option value={null}>No project · session files</option>{#each projects as [id, name]}<option value={id}>{name}</option>{/each}</select></label>
          {#if draft.id}<fieldset disabled={busy}>
            <legend>Schedule</legend>
            <label class="check"><Toggle bind:checked={draft.schedule.enabled} />Run on a schedule</label>
            {#if draft.schedule.enabled}
              <div class="schedule">
                <label>Repeat<select bind:value={draft.schedule.cadence}><option value="daily">Every day</option><option value="weekdays">Weekdays</option><option value="weekly">Every week</option></select></label>
                {#if draft.schedule.cadence === 'weekly'}<label>Day<select bind:value={draft.schedule.weekday}>{#each ['Monday','Tuesday','Wednesday','Thursday','Friday','Saturday','Sunday'] as day, i}<option value={i}>{day}</option>{/each}</select></label>{/if}
                <label>Time<input type="time" bind:value={draft.schedule.time} required /></label>
              </div>
              <p>Uses this computer’s time zone ({zone}). Runs while the computer is awake and the background service is available.</p>
              <p>A missed schedule runs once when the service returns.</p>
            {/if}
          </fieldset>{/if}
          <div class="actions">
            <button disabled={busy || !draft.name.trim() || !draft.instructions.trim()}>{draft.id ? 'Save changes' : 'Create agent'}</button>
            {#if draft.id}
            <button type="button" disabled={busy || !draft.name.trim() || !draft.instructions.trim()} onclick={() => action('thread')}>Start thread</button>
            <button type="button" disabled={busy || !draft.name.trim() || !draft.instructions.trim() || ['queued','running','waiting'].includes(run?.status)} onclick={() => action('run')}>Run now</button>
            {/if}
          </div>
        </form>
        {#if run}
          <div class="run">
            <h4>Latest run</h4><p>{run.status || 'Ready'}</p>
            {#if run.lastRun}<p>Last run: {formatted(run.lastRun)}</p>{/if}
            {#if run.nextRun && draft.schedule.enabled}<p>Next run: {formatted(run.nextRun)}</p>{/if}
            {#if run.error}<p role="status">{run.error}</p>{/if}
            {#if run.threadId}<button onclick={() => { onopen(run.threadId); onclose() }}>Open run thread</button>{/if}
          </div>
        {/if}
        {#if draft.id}<div class="actions secondary">
          <button disabled={busy} onclick={openFolder}>Open folder</button>
          <button disabled={busy} onclick={() => shareTemplate()}>Export template</button>
          <button disabled={busy} onclick={() => shareTemplate(true)}>Export for Grok Bot</button>
          {#if deleting}<button disabled={busy} onclick={() => action('delete')}>Confirm delete</button><button onclick={() => { deleting = false }}>Cancel</button>
          {:else}<button disabled={busy || ['queued','running','waiting'].includes(run?.status)} onclick={() => { deleting = true }}>Delete agent</button>{/if}
        </div>{/if}
        {#if status}<p role="status">{status}</p>{/if}
      </div>
  {/if}
</section>
<style>
  .link-form { padding: 0 24px 16px; display: flex; align-items: end; gap: 8px; }
  .link-form label { flex: 1; }
  .draft-avatar { display: flex; align-items: center; gap: 16px; }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--text-12); }
  .panel { grid-area: thread; z-index: 2; min-width: 0; min-height: 0; display: flex; flex-direction: column; background: var(--surface); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-panel); overflow: hidden; }
  .heading { display: flex; gap: 16px; align-items: center; min-width: 0; }
  h2 { font-size: var(--text-15); margin: 0; overflow-wrap: anywhere; }
  .catalog { padding: 12px 24px 24px; overflow: auto; display: grid; grid-template-columns: repeat(auto-fill,minmax(210px,1fr)); align-content: start; gap: 16px; }
  .card { min-height: 174px; display: flex; flex-direction: column; align-items: start; justify-content: center; gap: 10px; padding: 20px; text-align: left; overflow-wrap: anywhere; }
  .new-card { align-items: center; border-style: dashed; }
  .loading { padding: 24px; }
  .import { position: relative; overflow: hidden; padding: 5px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); cursor: pointer; }
  .import input { position: absolute; inset: 0; opacity: 0; cursor: pointer; }
  .import:focus-within { outline: 2px solid var(--ink); }
  h4 { margin: 0; font-size: var(--text-13); }

  header { display: flex; justify-content: space-between; align-items: center; padding: 18px 24px; }
  .body { max-width: 880px; width: 100%; box-sizing: border-box; overflow-y: auto; padding: 0 24px 24px; display: grid; gap: 16px; }
  form, label, .run { display: grid; gap: 8px; }
  form { gap: 16px; }
  label { font-size: var(--text-13); }
  p { margin: 0; font-size: var(--text-13); color: var(--muted); line-height: 1.5; }
  input, textarea, select { box-sizing: border-box; width: 100%; padding: 8px 10px; font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--paper); border: 1px solid var(--border); border-radius: var(--radius-control); }
  textarea { resize: vertical; line-height: 1.5; }
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 10px; cursor: pointer; }
  button:hover { background: var(--faint); }
  button:disabled { opacity: .5; cursor: default; }
  .quiet { background: transparent; border-color: transparent; line-height: 0; }
  .agent-row { display: flex; align-items: center; gap: 8px; width: 100%; text-align: left; padding: 8px; background: transparent; border-color: transparent; }
  .agent-row[aria-current] { background: var(--faint); }
  .avatar { display: grid; place-items: center; width: 24px; height: 24px; flex-shrink: 0; border-radius: var(--radius-control); background: var(--faint); }
  small { display: block; color: var(--muted); font-size: var(--text-12); text-transform: capitalize; }
  fieldset { border: 1px solid var(--border); border-radius: var(--radius-control); padding: 12px; display: grid; gap: 12px; }
  legend { font-size: var(--text-13); padding: 0 4px; }
  .check { display: flex; align-items: center; gap: 8px; }
  .schedule, .actions { display: flex; flex-wrap: wrap; gap: 8px; }
  .schedule label { flex: 1; }
  .secondary { border-top: 1px solid var(--border); padding-top: 16px; }
  @media (max-width: 680px) { header { padding: 12px; } .body { padding: 0 12px 16px; } .catalog { padding: 12px; } }
</style>
