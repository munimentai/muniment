<script>
  import { panelScroll } from './panel-scroll.js'
  import Toggle from './Toggle.svelte'
  import { tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import AgentAvatar from './AgentAvatar.svelte'
  import MemorySection from './MemorySection.svelte'
  import ModelPicker from './ModelPicker.svelte'
  import Button from './ui/Button.svelte'
  import { newAvatar } from './agent-avatar.js'
  import { runActive, runStatusLabel, templateNote } from './agent-labels.js'
  import { thinkingLabel } from './thinking-level.js'
  import { agentMachine, agentRequest } from '../agents/agent-runtime.js'
  import { agentTools, toggleTool } from '../agents/agent-tools.js'
  import { MODES, PATH_OPERATIONS, RULE_KINDS, SHELLS, emptyRuleForm, ruleFromForm, ruleLabel } from '../agents/permission-rules.js'
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
  let { agent, tauri, projects = [], run, runs = [], agents = [], runtimeList = null, swarm = null, inventory = null, skills = [], servers = [], webhooks = true,
    onclose, onchange = async () => {}, ondelete, onopen, onrun = () => {}, onmanagemodels = () => {} } = $props()
  let editing = $state(null)
  let value = $state('')
  let busy = $state(false)
  let error = $state('')
  let deleting = $state(false)
  let removeMemory = $state(false)
  let memoryOpen = $state(false)
  let modelOpen = $state(false)
  let ruleForm = $state(null)
  let moving = $state(false)
  let notice = $state('')
  let panel
  const local = $derived((agent.runtimeId ?? LOCAL_RUNTIME_ID) === LOCAL_RUNTIME_ID)
  const others = $derived(agents.filter((entry) => entry.id !== agent.id))
  const permissions = $derived(agent.permissions ?? { mode: 'ask', always_allow: [], ask_first: [] })
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
  // elsewhere stay. A null schedule leaves the routines as they are.
  async function saveAgent(patch) {
    const { agent: latest } = await agentRequest(tauri, agent.runtimeId, 'agent.read', { id: agent.id })
    await agentRequest(tauri, agent.runtimeId, 'agent.save', { agent: { ...latest, ...patch, schedule: null } })
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
  function addRule(event) {
    event.preventDefault()
    let request
    try { request = ruleFromForm(ruleForm) } catch (e) { error = e.message; return }
    const list = ruleForm.list
    ruleForm = null
    void change({ permissions: { ...permissions, [list]: [...(permissions[list] ?? []), request] } })
  }
  function removeRule(list, index) {
    void change({ permissions: { ...permissions, [list]: permissions[list].filter((_, at) => at !== index) } })
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
  <header data-panel-header><h2>Agent profile</h2><button data-panel-control class="quiet" aria-label="Collapse agent profile" onclick={onclose}><LucideIcon name="panel-right-close" size={16} /></button></header>
  <div class="body" use:panelScroll>
    {#if agent.replica}
      <div class="avatar"><AgentAvatar {agent} size={92} /></div>
      <section class="field"><div class="field-heading"><h3>Name</h3></div><p class="name">{agent.name}</p></section>
      {#if agent.label}<section class="field"><div class="field-heading"><h3>Job title</h3></div><p>{agent.label}</p></section>{/if}
      {@render primaryMachine()}
      <p class="muted">Its instructions, tools, routines and memory live on {primary.name}. Pair this device with that machine to change them.</p>
    {:else}
    <div class="avatar"><AgentAvatar {agent} size={92} /><button class="edit quiet" aria-label="Generate another avatar" disabled={busy} onclick={() => change({ avatar: newAvatar() })}><LucideIcon name="refresh-cw" size={14} /></button></div>
    {#each [['name', 'Name'], ['label', 'Job title'], ['instructions', 'Instructions'], ...(local ? [['projectId', 'Project']] : [])] as [field, label]}
      <section class="field">
        <div class="field-heading"><h3>{label}</h3>{#if editing !== field}<button class="edit quiet" disabled={busy} aria-label={`Edit ${fieldLabel(field)}`} onclick={() => edit(field)}><LucideIcon name="pencil" size={14} /></button>{/if}</div>
        {#if editing === field}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <form aria-label={`Edit ${label}`} onsubmit={event => { event.preventDefault(); void save() }} onkeydown={event => { if (event.key === 'Escape' && !busy) { event.stopPropagation(); void endEdit() } }}>
            {#if field === 'name' || field === 'label'}<input aria-label={label} bind:value maxlength={field === 'name' ? 100 : 120} required={field === 'name'} disabled={busy} />
            {:else if field === 'instructions'}<textarea aria-label="Instructions" rows="12" bind:value required disabled={busy}></textarea><small>Saving can propose routines from schedules in the instructions.</small>
            {:else}<select aria-label="Project" bind:value disabled={busy}><option value="">No project</option>{#each projects as [id, name]}<option value={id}>{name}</option>{/each}</select><small>New work uses this project. Existing files stay in their current folder.</small>{/if}
            <div class="actions"><button disabled={busy || (['name','instructions'].includes(field) && !value.trim())}>Save</button><button type="button" disabled={busy} onclick={endEdit}>Cancel</button></div>
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

    <section class="field" aria-label="Tools">
      <div class="field-heading"><h3>Tools</h3></div>
      <ul class="toggles">{#each agentTools(agent) as [id, label] (id)}<li><span>{label}<span class="record id">{id}</span></span><Toggle checked={!(agent.tools?.disabled ?? []).includes(id)} label={`${label} on`} disabled={busy} onchange={(on) => change({ tools: toggleTool(agent, id, on) })} /></li>{/each}</ul>
    </section>

    <section class="field" aria-label="Skills">
      <div class="field-heading"><h3>Skills</h3></div>
      {#if skills.length}
        <ul>{#each skills as skill (skill.id)}<li><label class="check"><input type="checkbox" checked={(agent.skills ?? []).includes(skill.id)} disabled={busy} onchange={(event) => toggleSkill(skill.id, event.currentTarget.checked)} />{skill.name}</label></li>{/each}</ul>
      {:else}<p class="muted">No skills installed. Add them in Extend.</p>{/if}
      {#each (agent.skills ?? []).filter((id) => !skills.some((skill) => skill.id === id)) as id (id)}<p class="record muted">{id} is not installed here.</p>{/each}
    </section>

    <section class="field" aria-label="Permissions">
      <div class="field-heading"><h3>Permissions</h3></div>
      <label class="inline">Mode<select aria-label="Permission mode" value={permissions.mode} disabled={busy} onchange={(event) => change({ permissions: { ...permissions, mode: event.currentTarget.value } })}>{#each MODES as [mode, label] (mode)}<option value={mode}>{label}</option>{/each}</select></label>
      <small>{MODES.find(([mode]) => mode === permissions.mode)?.[2]}</small>
      {#each [['always_allow', 'Always allow'], ['ask_first', 'Ask first']] as [list, heading] (list)}
        <h4>{heading}</h4>
        <ul class="rules" aria-label={heading}>
          {#each permissions[list] ?? [] as request, index (index)}<li><span class="record">{ruleLabel(request)}</span><button class="quiet icon" aria-label={`Remove ${heading.toLowerCase()} rule ${index + 1}`} disabled={busy} onclick={() => removeRule(list, index)}><LucideIcon name="x" size={14} /></button></li>{:else}<li class="muted">None</li>{/each}
        </ul>
      {/each}
      {#if ruleForm}
        <form class="rule-form" aria-label="New permission rule" onsubmit={addRule}>
          <label>List<select bind:value={ruleForm.list}><option value="always_allow">Always allow</option><option value="ask_first">Ask first</option></select></label>
          <label>Kind<select bind:value={ruleForm.kind}>{#each RULE_KINDS as [kind, label] (kind)}<option value={kind}>{label}</option>{/each}</select></label>
          {#if ruleForm.kind === 'command'}
            <label>Shell<select bind:value={ruleForm.shell}>{#each SHELLS as shell (shell)}<option value={shell}>{shell}</option>{/each}</select></label>
            <label>Command<input class="record" bind:value={ruleForm.command} spellcheck="false" /></label>
            <label>Folder<input class="record" bind:value={ruleForm.directory} placeholder="/full/path" spellcheck="false" /></label>
          {:else if ruleForm.kind === 'path'}
            <label>Action<select bind:value={ruleForm.operation}>{#each PATH_OPERATIONS as operation (operation)}<option value={operation}>{operation}</option>{/each}</select></label>
            <label>Path<input class="record" bind:value={ruleForm.path} placeholder="/full/path" spellcheck="false" /></label>
          {:else}
            <label>Tool<input class="record" bind:value={ruleForm.name} placeholder="mcp__server__tool" spellcheck="false" /></label>
            <label>Input<textarea class="record" rows="2" bind:value={ruleForm.input} spellcheck="false"></textarea></label>
          {/if}
          <small>Rules match exactly. A rule in Ask first wins over Always allow.</small>
          <div class="actions"><button disabled={busy}>Add rule</button><button type="button" onclick={() => { ruleForm = null }}>Cancel</button></div>
        </form>
      {:else}<div class="actions"><button disabled={busy} onclick={() => { ruleForm = { ...emptyRuleForm(), list: 'always_allow' } }}>Add rule</button></div>{/if}
    </section>

    <section class="field" aria-label="Messaging">
      <div class="field-heading"><h3>Messages to other agents</h3></div>
      <label class="check"><input type="radio" name={`allow-${agent.id}`} checked={allowList === 'any'} disabled={busy} onchange={() => change({ messaging: { allow: 'any' } })} />Any agent</label>
      <label class="check"><input type="radio" name={`allow-${agent.id}`} checked={allowList !== 'any'} disabled={busy} onchange={() => change({ messaging: { allow: [] } })} />Only these agents</label>
      {#if allowList !== 'any'}
        <ul class="nested">{#each others as other (`${other.runtimeId}:${other.id}`)}<li><label class="check"><input type="checkbox" checked={allowList.includes(other.id)} disabled={busy} onchange={(event) => allowAgent(other.id, event.currentTarget.checked)} /><AgentAvatar agent={other} size={16} />{other.name}</label></li>{:else}<li class="muted">No other agents.</li>{/each}</ul>
      {/if}
    </section>

    {@render primaryMachine()}

    <RoutinesPanel {agent} {tauri} {servers} {webhooks} disabled={busy} {onchange} />

    <section class="field" aria-label="Activity">
      <div class="field-heading"><h3>Activity</h3>{#if run?.status}<span class="record muted">{runStatusLabel(run.status)}</span>{/if}</div>
      <RunList runs={runs.slice(0, 20)} onopen={onopen ? (entry) => action(() => onopen(entry.thread_id)) : null} />
    </section>

    {#if local}<details class="template" bind:open={memoryOpen}><summary>Memory</summary>{#if memoryOpen}{#key agent.id}<MemorySection {tauri} agentId={agent.id} />{/key}{/if}</details>{/if}
    {#if agent.template}<details class="template"><summary>Imported template</summary><p>{templateNote(agent.template)}</p><pre>{JSON.stringify(agent.template, null, 2)}</pre></details>{/if}
    <div class="actions secondary">
      <Button disabled={busy || runActive(run?.status)} onclick={() => action(async () => { const answer = await agentRequest(tauri, agent.runtimeId, 'agent.run_now', { id: agent.id }); if (answer?.run) onrun({ ...answer.run, runtimeId: agent.runtimeId ?? LOCAL_RUNTIME_ID }); await onchange() })}>Run now</Button>
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
  .id { margin-left: 6px; color: var(--muted); }
  .muted { color: var(--muted); font-size: var(--text-12); }
  .machine { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .offline { color: var(--ochre); font: var(--text-12) var(--font-mono); }
  .synced { display: inline-flex; align-items: center; gap: 4px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  ul { display: grid; gap: 4px; margin: 0; padding: 0; list-style: none; }
  .toggles li, .rules li { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-width: 0; font-size: var(--text-13); }
  .rules li .record { min-width: 0; overflow-wrap: anywhere; }
  .nested { padding-left: 22px; }
  .rule-form { padding: 8px; border: 1px solid var(--border); border-radius: var(--radius-control); }
  small { color: var(--muted); line-height: 1.5; }
  [role="alert"] { margin-top: 8px; color: var(--oxide); font-size: var(--text-12); }
  @media (hover: none) { .edit { opacity: 1; } }
</style>
