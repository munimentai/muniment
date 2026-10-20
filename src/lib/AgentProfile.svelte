<script>
  import { panelScroll } from './panel-scroll.js'
  import Toggle from './Toggle.svelte'
  import { onMount, tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import AgentAvatar from './AgentAvatar.svelte'
  import MemorySection from './MemorySection.svelte'
  import ModelPicker from './ModelPicker.svelte'
  import Button from './ui/Button.svelte'
  import DisclosureSummary from './ui/DisclosureSummary.svelte'
  import { agentColor, newAvatar } from './agent-avatar.js'
  import { runActive, runStatusLabel, templateNote, whenLabel } from './agent-labels.js'
  import { thinkingLabel } from './thinking-level.js'
  import { agentMachine, agentRequest } from '../agents/agent-runtime.js'
  import { emptyCatalog, missingSkills, toggleTool } from '../agents/agent-tools.js'
  import { findMode, loadModes } from '../agents/permission-modes.js'
  import { activityLabel, activityState } from '../agents/activity.js'
  import ReplayList from '../agents/ReplayList.svelte'
  import PermissionRules from '../agents/PermissionRules.svelte'
  import MachineMark from '../agents/MachineMark.svelte'
  import RoutinesPanel from '../agents/RoutinesPanel.svelte'
  import RunList from '../agents/RunList.svelte'
  import MoveDialog from '../agents/MoveDialog.svelte'
  import { exportAgent } from '../agents/import.js'
  import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'

  // The agent lives on the machine `agent.runtimeId` names, and every change
  // is an agent operation there. The runtime validates and stores each one.
  // An agent the app sees only through the swarm directory (`replica`) shows
  // its profile, its primary machine and a move, since its settings live on
  // a machine this device does not reach.
  // `catalog` is core's `agent.tool_catalog` from the agent's own machine:
  // the tools, skills and MCP servers it can offer this agent.
  let { agent, tauri, projects = [], run, runs = [], threads = [], agents = [], runtimeList = null, swarm = null, inventory = null, catalog = emptyCatalog(), activity = undefined, webhooks = true,
    onclose, onchange = async () => {}, ondelete, onopen, onopenrun = null, onactivity = null, onrun = () => {}, onmanagemodels = () => {} } = $props()
  let editing = $state(null)
  let value = $state('')
  let busy = $state(false)
  let error = $state('')
  let deleting = $state(false)
  let removeMemory = $state(false)
  let memoryOpen = $state(false)
  let modelOpen = $state(false)
  let moving = $state(false)
  let notice = $state('')
  let panel
  const local = $derived((agent.runtimeId ?? LOCAL_RUNTIME_ID) === LOCAL_RUNTIME_ID)
  const others = $derived(agents.filter((entry) => entry.id !== agent.id))
  const permissions = $derived(agent.permissions ?? { always_allow: [], ask_first: [] })
  // Core's `permission.modes` from the agent's machine: undefined while it
  // loads, null when the machine lists none. An agent saves a mode only
  // once one is chosen, and follows the machine's default until then.
  let modes = $state(undefined)
  onMount(() => {
    if (agent.replica) return
    let alive = true
    void loadModes(tauri, agent.runtimeId).then((value) => { if (alive) modes = value })
    return () => { alive = false }
  })
  const shownMode = $derived(findMode(modes, permissions.mode ?? modes?.default_agent_mode))
  let disabledTool = $state('')
  const allowList = $derived(agent.messaging?.allow ?? 'any')
  const primary = $derived(agent.primaryMachine ?? agentMachine(runtimeList, { runtimeId: agent.runtimeId }))
  const movable = $derived((swarm?.members?.length ?? 0) > 1)
  const thinkingLevels = ['off', 'minimal', 'low', 'medium', 'high', 'xhigh']
  let instructionsOpen = $state(false)
  const fieldLabel = (field) => field === 'projectId' ? 'project' : field === 'label' ? 'job title' : field

  async function edit(field) {
    editing = field
    value = agent[field] ?? ''
    error = ''
    await tick()
    panel?.querySelector('form input, form textarea, form select')?.focus()
  }
  async function endEdit() {
    const field = editing
    editing = null
    await tick()
    panel?.querySelector(`[aria-label="Edit ${fieldLabel(field)}"]`)?.focus()
  }
  async function action(work) {
    busy = true; error = ''
    try { await work() } catch (e) { error = String(e?.message ?? e) }
    finally { busy = false }
  }
  // Reads the agent from its machine before applying a change, so edits made
  // elsewhere stay, such as a rule an approval saved. A patch may be a
  // function of that latest agent. A null schedule leaves the routines as they are.
  async function saveAgent(patch) {
    const { agent: latest } = await agentRequest(tauri, agent.runtimeId, 'agent.read', { id: agent.id })
    const changes = typeof patch === 'function' ? patch(latest) : patch
    await agentRequest(tauri, agent.runtimeId, 'agent.save', { agent: { ...latest, ...changes, schedule: null } })
    await onchange()
  }
  const change = (patch) => action(() => saveAgent(patch))
  async function save() {
    await action(async () => {
      await saveAgent({ [editing]: editing === 'projectId' ? value || null : value.trim() })
      busy = false
      await endEdit()
    })
  }
  function chooseModel(provider, choice) {
    modelOpen = false
    void change({ model: { provider, id: choice, ...(agent.model?.thinking ? { thinking: agent.model.thinking } : {}) } })
  }
  function chooseMode(id) {
    void change((latest) => {
      const { mode: _saved, ...rules } = latest?.permissions ?? permissions
      return { permissions: id ? { ...rules, mode: id } : rules }
    })
  }
  function disableTool(event) {
    event.preventDefault()
    const id = disabledTool.trim()
    if (!id) return
    disabledTool = ''
    void change({ tools: toggleTool(agent, id, false) })
  }
  function allowAgent(id, allowed) {
    const current = allowList === 'any' ? [] : allowList
    void change({ messaging: { allow: allowed ? [...new Set([...current, id])] : current.filter((entry) => entry !== id) } })
  }
  function toggleSkill(id, on) {
    const current = agent.skills ?? []
    void change({ skills: on ? [...new Set([...current, id])] : current.filter((entry) => entry !== id) })
  }
</script>
<aside data-panel="agent" class="profile" aria-label="Agent profile" bind:this={panel}>
  <!-- The header carries the accent edge in the agent's color, as the title pill does. -->
  <header data-panel-header data-accent-edge style:--edge-color={agentColor(agent)}><h2>Agent profile</h2><button data-panel-control class="quiet" aria-label="Collapse agent profile" onclick={onclose}><LucideIcon name="panel-right-close" size={16} /></button></header>
  <div class="body" use:panelScroll>
    {#if agent.replica}
      <div class="avatar"><AgentAvatar {agent} {activity} size={92} /></div>
      {#if activity !== undefined}<p class="state record" data-state={activityState(activity)}>{activityLabel(activity)}</p>{/if}
      <section class="field"><div class="field-heading"><h3>Name</h3></div><p class="name">{agent.name}</p></section>
      {#if agent.label}<section class="field"><div class="field-heading"><h3>Job title</h3></div><p>{agent.label}</p></section>{/if}
      {@render primaryMachine()}
      <p class="muted">Its instructions, tools, routines and memory live on {primary.name}. Pair this device with that machine to change them.</p>
    {:else}
    <div class="avatar"><AgentAvatar {agent} {activity} size={92} /><button class="edit quiet" aria-label="Generate another avatar" disabled={busy} onclick={() => change({ avatar: newAvatar() })}><LucideIcon name="refresh-cw" size={14} /></button></div>
    {#if activity !== undefined}<p class="state record" data-state={activityState(activity)}>{activityLabel(activity)}</p>{/if}
    {#each [['name', 'Name'], ['label', 'Job title'], ['instructions', 'Instructions'], ...(local ? [['projectId', 'Project']] : [])] as [field, label]}
      <section class="field">
        <div class="field-heading"><h3>{label}</h3>{#if editing !== field}<button class="edit quiet" disabled={busy} aria-label={`Edit ${fieldLabel(field)}`} onclick={() => edit(field)}><LucideIcon name="pencil" size={14} /></button>{/if}</div>
        {#if editing === field}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <form aria-label={`Edit ${label}`} onsubmit={event => { event.preventDefault(); void save() }} onkeydown={event => { if (event.key === 'Escape' && !busy) { event.stopPropagation(); void endEdit() } }}>
            {#if field === 'name' || field === 'label'}<input aria-label={label} bind:value maxlength={field === 'name' ? 100 : 120} required={field === 'name'} disabled={busy} />
            {:else if field === 'instructions'}<textarea aria-label="Instructions" rows="12" bind:value required disabled={busy}></textarea><small>Saving can propose routines from schedules in the instructions.</small>
            {:else}<select aria-label="Project" bind:value disabled={busy}><option value="">No project</option>{#each projects as [id, name]}<option value={id}>{name}</option>{/each}</select><small>New work uses this project. Existing files stay in their current folder.</small>{/if}
            <div class="actions"><button type="submit" data-variant="primary" disabled={busy || (['name','instructions'].includes(field) && !value.trim())}>Save</button><button type="button" disabled={busy} onclick={endEdit}>Cancel</button></div>
          </form>
        {:else}
          {#if field === 'instructions' && (agent.instructions || '').length > 400}<p class:clamped={!instructionsOpen}>{agent.instructions}</p><button class="more" aria-expanded={instructionsOpen} onclick={() => { instructionsOpen = !instructionsOpen }}>{instructionsOpen ? 'Show less' : 'Show all'}</button>
          {:else}<p class:name={field === 'name'}>{field === 'projectId' ? projects.find(([id]) => id === agent.projectId)?.[1] || 'No project' : agent[field] || 'Not set'}</p>{/if}
        {/if}
      </section>
    {/each}

    <section class="field" aria-label="Model">
      <div class="field-heading"><h3>Model</h3>{#if agent.model}<button class="quiet link" disabled={busy} onclick={() => change({ model: null })}>Use default</button>{/if}</div>
      <div class="model-anchor">
        <button class="chip" aria-haspopup="dialog" aria-expanded={modelOpen} disabled={busy} onclick={() => { modelOpen = !modelOpen }}>{#if agent.model}<span class="record">{agent.model.provider} · {agent.model.id}</span>{:else}Default model{/if}</button>
        {#if modelOpen}<div class="model-popover"><ModelPicker {inventory} current={agent.model ? { provider: agent.model.provider, model: agent.model.id } : null} onchoose={chooseModel} onmanage={() => { modelOpen = false; onmanagemodels() }} onrouting={() => { modelOpen = false; onmanagemodels() }} onclose={() => { modelOpen = false }} /></div>{/if}
      </div>
      {#if agent.model}<label class="inline">Thinking<select aria-label="Thinking" value={agent.model.thinking ?? ''} disabled={busy} onchange={(event) => change({ model: { provider: agent.model.provider, id: agent.model.id, ...(event.currentTarget.value ? { thinking: event.currentTarget.value } : {}) } })}><option value="">Model default</option>{#each thinkingLevels as level (level)}<option value={level}>{thinkingLabel(level)}</option>{/each}</select></label>{/if}
    </section>

    <section class="field" aria-label="Skills">
      <div class="field-heading"><h3>Skills</h3></div>
      {#if catalog.skills.length}
        <ul>{#each catalog.skills as skill (skill.id)}<li><label class="check skill"><input type="checkbox" checked={(agent.skills ?? []).includes(skill.id)} disabled={busy} onchange={(event) => toggleSkill(skill.id, event.currentTarget.checked)} /><span>{skill.name}{#if skill.description}<small>{skill.description}</small>{/if}</span></label></li>{/each}</ul>
      {:else if catalog.loaded && !catalog.error}<p class="muted">{primary.name} has no skills installed. Add them in Tools.</p>{/if}
      {#if catalog.loaded && !catalog.error}{#each missingSkills(agent, catalog) as id (id)}<p class="record muted">{id} is not installed on {primary.name}.</p>{/each}{/if}
    </section>

    <section class="field" aria-label="Permissions">
      <div class="field-heading"><h3>Permissions</h3></div>
      {#if modes === undefined}<p class="muted" role="status">Loading modes…</p>
      {:else if !modes}<p class="muted">Update {primary.name} to choose a mode.</p>
      {:else}
        <label class="inline">Mode<select aria-label="Permission mode" value={permissions.mode ?? ''} disabled={busy} onchange={(event) => chooseMode(event.currentTarget.value)}>
          <option value="">Default ({findMode(modes, modes.default_agent_mode)?.title ?? modes.default_agent_mode})</option>
          {#each modes.modes as mode (mode.id)}<option value={mode.id} disabled={mode.available === false && mode.id !== permissions.mode}>{mode.title}</option>{/each}
        </select></label>
        {#if shownMode}<small>{shownMode.available === false && shownMode.reason ? shownMode.reason : shownMode.description}</small>{/if}
      {/if}
      <PermissionRules rules={permissions} {busy} onsave={(list, requests) => change((latest) => ({ permissions: { ...(latest?.permissions ?? permissions), [list]: requests } }))} />
    </section>

    <details class="field advanced">
      <DisclosureSummary>Advanced</DisclosureSummary>
      <section class="advanced-body" aria-label="Disabled tools">
        <h4>Disabled tools (deprecated)</h4>
        <small>Modes and rules replace this list. These tools stay hidden from the agent.</small>
        <ul class="rules" aria-label="Disabled tools">
          {#each agent.tools?.disabled ?? [] as id (id)}<li><span class="record">{id}</span><button class="quiet icon" aria-label={`Turn ${id} on again`} disabled={busy} onclick={() => change({ tools: toggleTool(agent, id, true) })}><LucideIcon name="x" size={14} /></button></li>{:else}<li class="muted">None</li>{/each}
        </ul>
        <form class="inline" aria-label="Disable a tool" onsubmit={disableTool}>
          <input class="record" aria-label="Tool id" list={`agent-${agent.id}-tools`} bind:value={disabledTool} placeholder="bash" spellcheck="false" />
          <datalist id={`agent-${agent.id}-tools`}>{#each catalog.builtin as tool (tool.id)}<option value={tool.id}>{tool.label}</option>{/each}</datalist>
          <button disabled={busy || !disabledTool.trim()}>Disable</button>
        </form>
      </section>
    </details>

    <section class="field" aria-label="Messaging">
      <div class="field-heading"><h3>Messages to other agents</h3></div>
      <label class="check"><input type="radio" name={`allow-${agent.id}`} checked={allowList === 'any'} disabled={busy} onchange={() => change({ messaging: { allow: 'any' } })} />Any agent</label>
      <label class="check"><input type="radio" name={`allow-${agent.id}`} checked={allowList !== 'any'} disabled={busy} onchange={() => change({ messaging: { allow: [] } })} />Only these agents</label>
      {#if allowList !== 'any'}
        <ul class="nested">{#each others as other (`${other.runtimeId}:${other.id}`)}<li><label class="check"><input type="checkbox" checked={allowList.includes(other.id)} disabled={busy} onchange={(event) => allowAgent(other.id, event.currentTarget.checked)} /><AgentAvatar agent={other} size={16} />{other.name}</label></li>{:else}<li class="muted">No other agents.</li>{/each}</ul>
      {/if}
    </section>

    {@render primaryMachine()}

    <RoutinesPanel {agent} {tauri} {catalog} {webhooks} disabled={busy} {onchange} />

    <section class="field" aria-label="Activity">
      <div class="field-heading"><h3>Activity</h3>{#if run?.status}<span class="record muted">{runStatusLabel(run.status)}</span>{/if}</div>
      <RunList runs={runs.slice(0, 20)} empty="No runs yet." onopen={onopen ? (entry) => action(() => onopen(entry.thread_id)) : null} />
    </section>

    <ReplayList {tauri} {agent} {runs} disabled={busy} {onrun} onopen={onopenrun} {onactivity} />

    {#if threads.length > 1}
      <section class="field" aria-label="Conversations">
        <div class="field-heading"><h3>Conversations</h3><span class="record muted">{threads.length}</span></div>
        <ul class="threads">{#each threads as thread (thread.thread_id)}<li><button class="thread-link" disabled={busy || !onopen} onclick={() => action(() => onopen(thread.thread_id))}><span>{thread.title || 'Untitled'}</span><span class="record muted">{thread.primary ? 'Primary · ' : ''}{whenLabel(thread.updated_at)}</span></button></li>{/each}</ul>
      </section>
    {/if}

    {#if local}<details class="template" bind:open={memoryOpen}><summary>Memory</summary>{#if memoryOpen}{#key agent.id}<MemorySection {tauri} agentId={agent.id} />{/key}{/if}</details>{/if}
    {#if agent.template}<details class="template"><summary>Imported template</summary><p>{templateNote(agent.template)}</p><pre>{JSON.stringify(agent.template, null, 2)}</pre></details>{/if}
    <div class="actions secondary">
      <Button variant="primary" disabled={busy || runActive(run?.status)} onclick={() => action(async () => { const answer = await agentRequest(tauri, agent.runtimeId, 'agent.run_now', { id: agent.id }); if (answer?.run) onrun({ ...answer.run, runtimeId: agent.runtimeId ?? LOCAL_RUNTIME_ID }); await onchange() })}>Run now</Button>
      {#if local}<Button disabled={busy} onclick={() => action(() => tauri.invoke('agent_open', { id: agent.id }))}>Open folder</Button>{/if}
      <Button disabled={busy} onclick={() => action(() => exportAgent(tauri, agent, 'muniment'))}>Export for Muniment</Button>
      <Button disabled={busy} onclick={() => action(() => exportAgent(tauri, agent, 'grok_bot'))}>Export for Grok Bot</Button>
      {#if deleting}
        <div class="confirm" role="group" aria-label="Delete agent">
          <label class="check"><input type="checkbox" bind:checked={removeMemory} disabled={busy} />Also delete its memory</label>
          <div class="actions"><Button disabled={busy} onclick={() => action(async () => { await agentRequest(tauri, agent.runtimeId, 'agent.delete', { id: agent.id, remove_memory: removeMemory }); ondelete() })}>Confirm delete</Button><Button disabled={busy} onclick={() => { deleting = false; removeMemory = false }}>Cancel</Button></div>
        </div>
      {:else}<Button disabled={busy || runActive(run?.status)} onclick={() => { deleting = true }}>Delete agent</Button>{/if}
    </div>
    {/if}
    {#if notice}<p class="muted" role="status">{notice}</p>{/if}
    {#if error}<p role="alert">{error}</p>{/if}
  </div>
</aside>

{#snippet primaryMachine()}
  <section class="field" aria-label="Primary machine">
    <div class="field-heading"><h3>Primary machine</h3>{#if movable}<button class="quiet link" disabled={busy} onclick={() => { moving = true; notice = '' }}>Move</button>{/if}</div>
    <p class="machine"><MachineMark machine={primary} />{#if agent.online === false}<span class="offline">Offline</span>{/if}{#if agent.replica}<span class="synced"><LucideIcon name="refresh-cw" size={12} />Synced copy</span>{/if}</p>
    <small>Routines and runs happen on this machine.</small>
  </section>
{/snippet}

{#if moving}
  <MoveDialog {tauri} {agent} {swarm} {runtimeList} onclose={() => { moving = false }} onmoved={async (line) => { notice = line; await onchange() }} />
{/if}
<style>
  .profile { grid-area: rail;   display: flex; flex-direction: column; overflow: hidden;     }

  h2 { margin: 0; font-size: var(--text-13); }
  .body { overflow: auto; padding: 8px 16px 24px; }
  .avatar { position: relative; display: flex; justify-content: center; margin-bottom: 8px; }
  .avatar .edit { position: absolute; bottom: 0; left: calc(50% + 50px); }
  .avatar:hover .edit, .avatar:focus-within .edit { opacity: 1; }
  .template { margin-top: 12px; font-size: var(--text-12); }
  pre { white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--text-12); }
  .field { display: grid; gap: 6px; padding: 12px 0; border-bottom: 1px solid var(--border); }
  .field-heading { display: flex; justify-content: space-between; align-items: center; gap: 8px; min-height: 24px; }
  h3 { margin: 0; color: var(--muted); font-size: var(--text-12); font-weight: 500; }
  h4 { margin: 6px 0 0; font-size: var(--text-12); font-weight: 600; }
  p { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--text-13); line-height: 1.5; }
  p.name { font-size: var(--text-15); font-weight: 600; }
  p.clamped { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 8; overflow: hidden; }
  .more { justify-self: start; }
  .edit { opacity: 0; }
  .field:hover .edit, .field:focus-within .edit { opacity: 1; }
  form, label { display: grid; gap: 8px; font-size: var(--text-12); }
  .inline { grid-template-columns: auto 1fr; align-items: center; }
  input, textarea, select { box-sizing: border-box; width: 100%; min-width: 0; padding: 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; }
  input[type="checkbox"], input[type="radio"] { width: auto; margin: 0; }
  textarea { resize: vertical; line-height: 1.5; }
  .check { display: flex; align-items: center; gap: 8px; min-height: 24px; font-size: var(--text-13); }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .secondary { margin-top: 16px; }
  .confirm { display: grid; gap: 8px; flex-basis: 100%; }
  button { font: inherit; font-size: var(--text-12); color: var(--ink); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 8px; background: transparent; cursor: pointer; }
  button:hover { background: var(--faint); }
  button:disabled { opacity: .5; cursor: default; }
  .quiet { border-color: transparent; padding: 4px; line-height: 0; }
  .link { line-height: 1.4; padding: 2px 4px; }
  .icon { display: grid; place-items: center; min-width: 24px; min-height: 24px; }
  .chip { justify-self: start; max-width: 100%; text-align: left; overflow-wrap: anywhere; }
  .model-anchor { position: relative; display: grid; }
  .model-popover { position: absolute; top: calc(100% + 6px); right: 0; left: 0; z-index: 5; }
  .record { font-family: var(--font-mono); font-size: var(--text-12); }
  .muted { color: var(--muted); font-size: var(--text-12); }
  .machine { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .state { display: -webkit-box; margin: 0 0 4px; overflow: hidden; color: var(--muted); text-align: center; overflow-wrap: anywhere; -webkit-box-orient: vertical; -webkit-line-clamp: 2; }
  .state[data-state="waiting"] { color: var(--ochre); }
  .state[data-state="error"] { color: var(--oxide); }
  .skill span { display: grid; gap: 2px; }
  .skill small { font-size: var(--text-12); }
  .threads { gap: 0; }
  .thread-link { display: flex; justify-content: space-between; gap: 8px; width: 100%; padding: 4px 6px; border: 0; text-align: left; font-size: var(--text-13); }
  .thread-link span:first-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .offline { color: var(--ochre); font: var(--text-12) var(--font-human); }
  .synced { display: inline-flex; align-items: center; gap: 4px; color: var(--muted); font: var(--text-12) var(--font-human); }
  ul { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; }
  .rules li { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-width: 0; font-size: var(--text-13); }
  .nested { padding-left: 22px; }
  .advanced-body { display: grid; gap: 6px; padding-top: 6px; }
  .advanced form.inline { grid-template-columns: 1fr auto; }
  small { color: var(--muted); line-height: 1.5; }
  [role="alert"] { margin-top: 8px; color: var(--oxide); font-size: var(--text-12); }
  @media (hover: none) { .edit { opacity: 1; } }
</style>
