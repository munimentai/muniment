<script>
  import { panelScroll } from './panel-scroll.js'
  import { onMount, tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import AgentAvatar from './AgentAvatar.svelte'
  import MemorySection from './MemorySection.svelte'
  import ModelPicker from './ModelPicker.svelte'
  import ThinkingPicker from './ThinkingPicker.svelte'
  import ProviderLogo from './ProviderLogo.svelte'
  import Button from './ui/Button.svelte'
  import SegmentedField from './ui/SegmentedField.svelte'
  import ComposerMenuButton from '../composer/ComposerMenuButton.svelte'
  import ModePicker from '../composer/ModePicker.svelte'
  import { agentColor, newAvatar } from './agent-avatar.js'
  import { runActive, templateNote, whenLabel } from './agent-labels.js'
  import { modelThinking, thinkingLabel } from './thinking-level.js'
  import { agentMachine, agentRequest } from '../agents/agent-runtime.js'
  import { emptyCatalog, missingSkills } from '../agents/agent-tools.js'
  import { findMode, loadModes, modeIcon } from '../agents/permission-modes.js'
  import { activityLabel, activityState } from '../agents/activity.js'
  import { memberMachine } from '../agents/swarm.js'
  import { kindIcon } from '../runtimes/runtimes.js'
  import AgentDialog from '../agents/AgentDialog.svelte'
  import ChoiceMenu from '../agents/ChoiceMenu.svelte'
  import MachineMark from '../agents/MachineMark.svelte'
  import RoutinesPanel from '../agents/RoutinesPanel.svelte'
  import RunHistory from '../agents/RunHistory.svelte'
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
    onclose, onchange = async () => {}, ondelete, onopen, onopenrun = null, onrun = () => {}, onmanagemodels = () => {}, onvisibility = () => {} } = $props()
  let editing = $state(null)
  let value = $state('')
  let busy = $state(false)
  let error = $state('')
  let deleting = $state(false)
  let removeMemory = $state(false)
  let memoryOpen = $state(false)
  // The one profile menu open: project, model, thinking, mode, machine or agents.
  let menu = $state(null)
  let moving = $state(null)
  let notice = $state('')
  let panel
  const local = $derived((agent.runtimeId ?? LOCAL_RUNTIME_ID) === LOCAL_RUNTIME_ID)
  const others = $derived(agents.filter((entry) => entry.id !== agent.id))
  const permissions = $derived(agent.permissions ?? {})
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
  const allowList = $derived(agent.messaging?.allow ?? 'any')
  const allowed = $derived(allowList === 'any' ? [] : others.filter((other) => allowList.includes(other.id)))
  const primary = $derived(agent.primaryMachine ?? agentMachine(runtimeList, { runtimeId: agent.runtimeId }))
  const movable = $derived((swarm?.members?.length ?? 0) > 1)
  const machines = $derived((swarm?.members ?? []).map((member) => {
    const machine = memberMachine(swarm, member.runtime_id, runtimeList)
    return { value: member.runtime_id, label: machine.name, icon: kindIcon(machine), tag: member.online ? '' : 'Offline' }
  }))
  const projectName = $derived(projects.find(([id]) => id === agent.projectId)?.[1] ?? '')
  const thinking = $derived(agent.model ? modelThinking(inventory, { provider: agent.model.provider, model: agent.model.id }) : null)
  const thinkingLevel = $derived(thinking ? (thinking.levels.includes(agent.model.thinking) ? agent.model.thinking : thinking.level) : null)
  // The conversations the history does not reach, such as a chat with the agent.
  const chats = $derived.by(() => {
    const reached = new Set(runs.map((entry) => entry.thread_id).filter(Boolean))
    return threads.filter((thread) => !thread.primary && !reached.has(thread.thread_id))
  })
  let instructionsOpen = $state(false)
  const fieldLabel = (field) => field === 'label' ? 'job title' : field

  // A click outside the open menu's row closes it.
  $effect(() => {
    if (!menu) return
    const name = menu
    const outside = (event) => { if (!event.target?.closest?.(`[data-profile-menu="${name}"]`)) menu = null }
    window.addEventListener('pointerdown', outside, true)
    return () => window.removeEventListener('pointerdown', outside, true)
  })
  const toggleMenu = (name) => { menu = menu === name ? null : name }
  async function closeMenu() {
    const name = menu
    menu = null
    await tick()
    panel?.querySelector(`[data-menu="agent-${name}"]`)?.focus()
  }

  async function edit(field) {
    editing = field
    value = agent[field] ?? ''
    error = ''
    await tick()
    panel?.querySelector('form input, form textarea')?.focus()
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
      await saveAgent({ [editing]: value.trim() })
      busy = false
      await endEdit()
    })
  }
  function chooseProject(id) {
    void closeMenu()
    if (id !== (agent.projectId ?? '')) void change({ projectId: id || null })
  }
  function chooseModel(provider, choice) {
    void closeMenu()
    void change({ model: { provider, id: choice, ...(agent.model?.thinking ? { thinking: agent.model.thinking } : {}) } })
  }
  function chooseThinking(level) {
    void closeMenu()
    if (level !== thinkingLevel) void change({ model: { provider: agent.model.provider, id: agent.model.id, thinking: level } })
  }
  function chooseMode(id) {
    void closeMenu()
    if (id === permissions.mode) return
    void change({ permissions: { mode: id } })
  }
  function chooseMachine(id) {
    void closeMenu()
    if (id !== agent.primaryRuntime) { moving = { destination: id }; notice = '' }
  }
  function allowAgent(id, on) {
    const current = allowList === 'any' ? [] : allowList
    void change({ messaging: { allow: on ? [...new Set([...current, id])] : current.filter((entry) => entry !== id) } })
  }
  function chooseAllow(next) {
    if ((next === 'any') !== (allowList === 'any')) void change({ messaging: { allow: next === 'any' ? 'any' : [] } })
  }
  function toggleSkill(id, on) {
    const current = agent.skills ?? []
    void change({ skills: on ? [...new Set([...current, id])] : current.filter((entry) => entry !== id) })
  }
  function openRun(entry) {
    if (entry.thread_id && onopen) return action(() => onopen(entry.thread_id))
    if (onopenrun) return action(() => onopenrun(entry))
  }
</script>
<aside data-panel="agent" class="profile" aria-label="Agent profile" bind:this={panel}>
  <!-- The header carries the accent edge in the agent's color, as the title pill does. -->
  <header data-panel-header data-accent-edge="top" style:--edge-color={agentColor(agent)}><h2>Agent profile</h2><button data-panel-control class="quiet" aria-label="Collapse agent profile" onclick={onclose}><LucideIcon name="panel-right-close" size={16} /></button></header>
  <div class="body" use:panelScroll>
    {#if agent.replica}
      <div class="avatar"><AgentAvatar {agent} {activity} size={92} /></div>
      {#if activity !== undefined}<p class="state record" data-state={activityState(activity)}>{activityLabel(activity)}</p>{/if}
      <section class="field"><div class="field-heading"><h3>Name</h3></div><p class="name">{agent.name}</p></section>
      {#if agent.label}<section class="field"><div class="field-heading"><h3>Job title</h3></div><p>{agent.label}</p></section>{/if}
      <div class="settings">{@render primaryMachine()}</div>
      <p class="muted">Its instructions, tools, routines and memory live on {primary.name}. Pair this device with that machine to change them.</p>
    {:else}
    <div class="avatar"><AgentAvatar {agent} {activity} size={92} /><button class="edit quiet" aria-label="Generate another avatar" disabled={busy} onclick={() => change({ avatar: newAvatar() })}><LucideIcon name="refresh-cw" size={14} /></button></div>
    {#if activity !== undefined}<p class="state record" data-state={activityState(activity)}>{activityLabel(activity)}</p>{/if}
    {#each [['name', 'Name'], ['label', 'Job title'], ['instructions', 'Instructions']] as [field, label]}
      <section class="field">
        <div class="field-heading"><h3>{label}</h3>{#if editing !== field}<button class="edit quiet" disabled={busy} aria-label={`Edit ${fieldLabel(field)}`} onclick={() => edit(field)}><LucideIcon name="pencil" size={14} /></button>{/if}</div>
        {#if editing === field}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <form aria-label={`Edit ${label}`} onsubmit={event => { event.preventDefault(); void save() }} onkeydown={event => { if (event.key === 'Escape' && !busy) { event.stopPropagation(); void endEdit() } }}>
            {#if field === 'instructions'}<textarea aria-label="Instructions" rows="12" bind:value required disabled={busy}></textarea><small>Saving can propose routines from schedules in the instructions.</small>
            {:else}<input aria-label={label} bind:value maxlength={field === 'name' ? 100 : 120} required={field === 'name'} disabled={busy} />{/if}
            <div class="actions"><button type="submit" data-variant="primary" disabled={busy || (['name','instructions'].includes(field) && !value.trim())}>Save</button><button type="button" disabled={busy} onclick={endEdit}>Cancel</button></div>
          </form>
        {:else}
          {#if field === 'instructions' && (agent.instructions || '').length > 400}<p class:clamped={!instructionsOpen}>{agent.instructions}</p><button class="more" aria-expanded={instructionsOpen} onclick={() => { instructionsOpen = !instructionsOpen }}>{instructionsOpen ? 'Show less' : 'Show all'}</button>
          {:else}<p class:name={field === 'name'}>{agent[field] || 'Not set'}</p>{/if}
        {/if}
      </section>
    {/each}

    <div class="settings">
      {#if local}
        <section class="setting" aria-label="Project" data-profile-menu="project">
          <h3>Project</h3>
          <span class="control"><ComposerMenuButton menu="agent-project" open={menu === 'project'} chevrons label={`Project: ${projectName || 'No project'}`} text={projectName || 'No project'} disabled={busy} onclick={() => toggleMenu('project')}>{#snippet icon()}<LucideIcon name="folder" size={14} />{/snippet}</ComposerMenuButton></span>
          {#if menu === 'project'}<ChoiceMenu label="Project" value={agent.projectId ?? ''} options={[{ value: '', label: 'No project' }, ...projects.map(([id, name]) => ({ value: id, label: name, icon: 'folder' }))]} note="New work uses this project. Existing files stay in their current folder." onchoose={chooseProject} onclose={closeMenu} />{/if}
        </section>
      {/if}
      <section class="setting" aria-label="Model" data-profile-menu={menu === 'thinking' ? 'thinking' : 'model'}>
        <h3>Model</h3>
        <span class="control">
          <ComposerMenuButton menu="agent-model" open={menu === 'model'} chevrons haspopup="dialog" label={`Model: ${agent.model ? agent.model.id : 'Default model'}`} text={agent.model ? agent.model.id : 'Default model'} disabled={busy} onclick={() => toggleMenu('model')}>{#snippet icon()}{#if agent.model}<ProviderLogo provider={agent.model.provider} size={14} />{:else}<LucideIcon name="cpu" size={14} />{/if}{/snippet}</ComposerMenuButton>
          {#if thinking}<ComposerMenuButton menu="agent-thinking" open={menu === 'thinking'} label={`Thinking level: ${thinkingLabel(thinkingLevel)}`} text={thinkingLabel(thinkingLevel)} disabled={busy} onclick={() => toggleMenu('thinking')}>{#snippet icon()}<LucideIcon name="brain" size={14} />{/snippet}</ComposerMenuButton>{/if}
        </span>
        {#if menu === 'model'}<ModelPicker {inventory} current={agent.model ? { provider: agent.model.provider, model: agent.model.id } : null} source="this agent" actions={agent.model ? [{ id: 'default', label: 'Use the default model' }] : []} onaction={() => { void closeMenu(); void change({ model: null }) }} onchoose={chooseModel} {onvisibility} onmanage={() => { menu = null; onmanagemodels() }} onrouting={() => { menu = null; onmanagemodels() }} onclose={closeMenu} />{/if}
        {#if menu === 'thinking' && thinking}<ThinkingPicker levels={thinking.levels} current={thinkingLevel} onchoose={chooseThinking} onclose={closeMenu} />{/if}
      </section>
      <section class="setting" aria-label="Permissions" data-profile-menu="mode">
        <h3>Permissions</h3>
        <span class="control">
          {#if modes === undefined}<span class="muted">Loading…</span>
          {:else if !modes}<span class="muted">Update {primary.name} to choose.</span>
          {:else}<ComposerMenuButton menu="agent-mode" open={menu === 'mode'} chevrons label={`Permissions: ${shownMode?.title ?? 'Choose'}`} text={shownMode?.title ?? 'Choose'} disabled={busy} onclick={() => toggleMenu('mode')}>{#snippet icon()}<LucideIcon name={modeIcon(shownMode)} size={14} />{/snippet}</ComposerMenuButton>{/if}
        </span>
        {#if menu === 'mode' && modes}<ModePicker modes={modes.modes} current={shownMode?.id} source={permissions.mode ? '' : 'default'} hint={false} onchoose={chooseMode} onclose={closeMenu} />{/if}
      </section>
      {@render primaryMachine()}
    </div>

    <section class="field" aria-label="Skills">
      <div class="field-heading"><h3>Skills</h3></div>
      {#if catalog.skills.length}
        <ul>{#each catalog.skills as skill (skill.id)}<li><label class="check skill"><input type="checkbox" checked={(agent.skills ?? []).includes(skill.id)} disabled={busy} onchange={(event) => toggleSkill(skill.id, event.currentTarget.checked)} /><span>{skill.name}{#if skill.description}<small>{skill.description}</small>{/if}</span></label></li>{/each}</ul>
      {:else if catalog.loaded && !catalog.error}<p class="muted">{primary.name} has no skills installed. Add them in Tools.</p>{/if}
      {#if catalog.loaded && !catalog.error}{#each missingSkills(agent, catalog) as id (id)}<p class="record muted">{id} is not installed on {primary.name}.</p>{/each}{/if}
    </section>

    <section class="field" aria-label="Messaging">
      <div class="field-heading"><h3>Messages to other agents</h3></div>
      <SegmentedField label="Messages to other agents" hideLabel value={allowList === 'any' ? 'any' : 'only'} options={[{ value: 'any', label: 'Any' }, { value: 'only', label: 'Only these' }]} onchange={chooseAllow} />
      {#if allowList !== 'any'}
        <div class="allowed" data-profile-menu="agents">
          {#each allowed as other (`${other.runtimeId}:${other.id}`)}<span class="agent-chip"><AgentAvatar agent={other} size={16} />{other.name}</span>{/each}
          <ComposerMenuButton menu="agent-agents" open={menu === 'agents'} chevrons label="Choose agents" text={allowed.length ? 'Change' : 'Choose agents'} disabled={busy} onclick={() => toggleMenu('agents')}>{#snippet icon()}<LucideIcon name="users" size={14} />{/snippet}</ComposerMenuButton>
          {#if menu === 'agents'}<ChoiceMenu label="Agents" multiple values={allowList} options={others.map((other) => ({ value: other.id, label: other.name, agent: other }))} empty="No other agents." onchoose={allowAgent} onclose={closeMenu} />{/if}
        </div>
      {/if}
    </section>

    <RoutinesPanel {agent} {tauri} {catalog} {webhooks} disabled={busy} {onchange} />

    <RunHistory {tauri} {agent} {runs} disabled={busy} {onrun} onopen={onopen || onopenrun ? openRun : null} />

    {#if chats.length}
      <section class="field" aria-label="Conversations">
        <div class="field-heading"><h3>Conversations</h3></div>
        <ul class="threads">{#each chats as thread (thread.thread_id)}<li><button class="thread-link" disabled={busy || !onopen} onclick={() => action(() => onopen(thread.thread_id))}><span>{thread.title || 'Untitled'}</span><span class="muted">{whenLabel(thread.updated_at)}</span></button></li>{/each}</ul>
      </section>
    {/if}

    {#if local}<section class="setting memory" aria-label="Memory"><h3>Memory</h3><button class="quiet link" onclick={() => { memoryOpen = true }}>Open memory</button></section>{/if}
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

<!-- The primary machine shows once the fleet has another machine to move to. A pick there starts the move. -->
{#snippet primaryMachine()}
  {#if movable || agent.replica}
    <section class="setting" aria-label="Primary machine" data-profile-menu="machine">
      <h3>Primary machine</h3>
      <span class="control">
        {#if agent.online === false}<span class="offline">Offline</span>{/if}
        {#if agent.replica}<span class="synced"><LucideIcon name="refresh-cw" size={12} />Synced copy</span>{/if}
        {#if movable}<ComposerMenuButton menu="agent-machine" open={menu === 'machine'} chevrons label={`Primary machine: ${primary.name}`} text={primary.name} disabled={busy} onclick={() => toggleMenu('machine')}>{#snippet icon()}<LucideIcon name={kindIcon(primary)} size={14} />{/snippet}</ComposerMenuButton>
        {:else}<MachineMark machine={primary} />{/if}
      </span>
      {#if menu === 'machine'}<ChoiceMenu label="Primary machine" value={agent.primaryRuntime} options={machines} note="Routines and runs happen on the primary machine." onchoose={chooseMachine} onclose={closeMenu} />{/if}
    </section>
  {/if}
{/snippet}

{#if moving}
  <MoveDialog {tauri} {agent} {swarm} {runtimeList} destination={moving.destination} onclose={() => { moving = null }} onmoved={async (line) => { notice = line; await onchange() }} />
{/if}
{#if memoryOpen}
  <AgentDialog title="Agent memory" panel="agent-memory" width={640} onclose={() => { memoryOpen = false }}>{#key agent.id}<MemorySection {tauri} agentId={agent.id} />{/key}</AgentDialog>
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
  .field { display: grid; gap: 4px; padding: 10px 0; border-bottom: 1px solid var(--border); }
  .field-heading { display: flex; justify-content: space-between; align-items: center; gap: 8px; min-height: 24px; }
  h3 { flex: none; margin: 0; color: var(--muted); font-size: var(--text-12); font-weight: 500; }
  /* One heading and its select per row, as the composer draws its menus. Each menu opens under its row. */
  .settings { display: grid; gap: 2px; padding: 8px 0; border-bottom: 1px solid var(--border); }
  .setting { position: relative; display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 28px; }
  .control { display: flex; flex-wrap: wrap; align-items: center; justify-content: flex-end; gap: 4px; min-width: 0; }
  .control > :global(button) { flex: 0 1 auto; min-width: 0; }
  .setting > :global([data-panel-variant="overlay"]), .allowed > :global([data-panel-variant="overlay"]) { position: absolute; z-index: 5; top: calc(100% + 4px); bottom: auto; left: auto; right: 0; max-height: 60vh; }
  .memory { padding: 6px 0; border-bottom: 1px solid var(--border); }
  p { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--text-13); line-height: 1.5; }
  p.name { font-size: var(--text-15); font-weight: 600; }
  p.clamped { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 8; overflow: hidden; }
  .more { justify-self: start; }
  .edit { opacity: 0; }
  .field:hover .edit, .field:focus-within .edit { opacity: 1; }
  form, label { display: grid; gap: 8px; font-size: var(--text-12); }
  input, textarea { box-sizing: border-box; width: 100%; min-width: 0; padding: 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; }
  input[type="checkbox"] { width: auto; margin: 0; }
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
  .record { font-family: var(--font-mono); font-size: var(--text-12); }
  .muted { color: var(--muted); font-size: var(--text-12); }
  .state { display: -webkit-box; margin: 0 0 4px; overflow: hidden; color: var(--muted); text-align: center; overflow-wrap: anywhere; -webkit-box-orient: vertical; -webkit-line-clamp: 2; }
  .state[data-state="waiting"] { color: var(--ochre); }
  .state[data-state="error"] { color: var(--oxide); }
  .skill span { display: grid; gap: 2px; }
  .skill small { font-size: var(--text-12); }
  .allowed { position: relative; display: flex; flex-wrap: wrap; align-items: center; gap: 4px; }
  .agent-chip { display: inline-flex; align-items: center; gap: 4px; min-height: 24px; padding: 0 8px 0 4px; border: 1px solid var(--border); border-radius: var(--radius-chip); font: var(--text-12) var(--font-human); }
  .threads { gap: 0; }
  .thread-link { display: flex; justify-content: space-between; gap: 8px; width: 100%; padding: 4px 6px; border: 0; text-align: left; font-size: var(--text-13); }
  .thread-link span:first-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .offline { color: var(--ochre); font: var(--text-12) var(--font-human); }
  .synced { display: inline-flex; align-items: center; gap: 4px; color: var(--muted); font: var(--text-12) var(--font-human); }
  ul { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; }
  small { color: var(--muted); line-height: 1.5; }
  [role="alert"] { margin-top: 8px; color: var(--oxide); font-size: var(--text-12); }
  @media (hover: none) { .edit { opacity: 1; } }
</style>
