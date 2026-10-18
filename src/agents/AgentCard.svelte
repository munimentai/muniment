<script>
  // One agent on the Agents page, laid out like the composer's model picker:
  // the avatar and the name across the top, a rail of panel icons down the
  // left, and the picked panel on the right. Each panel holds the settings
  // that edit in one step; the rest stay in the profile. Every change is an
  // agent operation on the agent's own machine, and core validates it.
  // A synced copy, or an agent whose machine does not answer, is read only.
  import { machineName as machineLabel } from '../runtimes/runtimes.js'
  import { tick, untrack } from 'svelte'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import { agentColor } from '../lib/agent-avatar.js'
  import StatusChip from '../lib/ui/StatusChip.svelte'
  import { runStatusTone } from '../lib/status-tone.js'
  import CatalogActions from '../lib/CatalogActions.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ProviderLogo from '../lib/ProviderLogo.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import Button from '../lib/ui/Button.svelte'
  import { panelScroll } from '../lib/panel-scroll.js'
  import { pickerGroups, providerName } from '../lib/provider-catalog.js'
  import { classifierProvider } from '../lib/classifier-catalog.js'
  import { runActive, runDuration, runStatusLabel, triggerLabel, whenLabel } from '../lib/agent-labels.js'
  import { agentRequest, saveAgentPatch } from './agent-runtime.js'
  import { TOOL_KINDS, agentToolChoice, projectTools as resolveTools } from './agent-tools.js'
  import InlineEdit from './InlineEdit.svelte'
  import MachineMark from './MachineMark.svelte'
  import { agentMode, modeIcon, modePermissions } from './permission-modes.js'
  import { isProposal, nextRunLabel, triggerSummary } from './routine-labels.js'

  // `modes` is core's `permission.modes` from the agent's machine: undefined
  // while it loads, null when the machine lists none. `projectTools` is the
  // tools of the agent's project, which the agent's choices sit over.
  let { agent, tauri, machine, inventory = null, latestRun = null, modes = undefined, archived = false, projectTools = null, projectName = '',
    onchat = () => {}, onprofile = () => {}, onaction = async () => {}, onchange = async () => {}, onrun = () => {}, onmanagemodels = () => {} } = $props()

  const PANELS = [
    { id: 'overview', label: 'Overview', icon: 'layout-dashboard' },
    { id: 'model', label: 'Model', icon: 'cpu' },
    { id: 'permissions', label: 'Permissions', icon: 'shield-check' },
    { id: 'tools', label: 'Tools', icon: 'wrench' },
    { id: 'routines', label: 'Routines', icon: 'clock' },
    { id: 'activity', label: 'Activity', icon: 'activity' },
  ]
  let panel = $state('overview')
  let tip = $state(null)
  let busy = $state(false)
  let error = $state('')
  let runs = $state({ list: [], loading: false, error: '' })
  let nameField = $state()

  const key = $derived(`${agent.runtimeId ?? 'local'}-${agent.id}`)
  const readOnly = $derived(!!agent.replica || agent.online === false)
  const machineName = $derived(machine ? machineLabel(machine) : 'its machine')
  const readOnlyNote = $derived(agent.online === false ? `Read only while ${machineName} is offline.` : `Synced copy. Edit it on ${machineName}.`)
  const currentMode = $derived(agentMode(agent, modes))
  const routines = $derived(agent.routines ?? [])
  const inUse = (model) => agent.model?.provider === model.provider && agent.model?.id === model.choice
  // The picker's groups and names: each provider's current models, and the
  // agent's model even when it sits under Legacy models.
  const groups = $derived(pickerGroups(inventory, '', { all: true }).map((group) => {
    const providerId = group.classifier ? classifierProvider(group.classifier) : group.id.replace(/^router:/, '')
    return {
      ...group,
      providerId,
      label: group.classifier ? 'Model router' : (providerName(providerId) === providerId ? group.name : providerName(providerId)),
      models: group.models.filter((model) => model.shown || inUse(model)),
    }
  }).filter((group) => group.models.length))

  export function editName() { return nameField?.open() }

  async function change(patch) {
    busy = true
    error = ''
    try { await saveAgentPatch(tauri, agent, patch); await onchange() }
    catch (failure) { error = String(failure?.message ?? failure) }
    finally { busy = false }
  }
  // The name and job title fields show their own refusal, so they throw it.
  async function saveField(patch) {
    await saveAgentPatch(tauri, agent, patch)
    await onchange()
  }
  function chooseModel(model) {
    if (model ? inUse(model) : !agent.model) return
    void change({ model: model ? { provider: model.provider, id: model.choice, ...(agent.model?.thinking ? { thinking: agent.model.thinking } : {}) } : null })
  }
  function chooseMode(mode) {
    if (mode.id === currentMode?.id || mode.available === false) return
    void change({ permissions: modePermissions(agent, mode) })
  }
  async function toggleRoutine(routine, enabled) {
    busy = true
    error = ''
    try { await agentRequest(tauri, agent.runtimeId, 'routine.save', { agent_id: agent.id, routine: { ...routine, enabled } }); await onchange() }
    catch (failure) { error = String(failure?.message ?? failure) }
    finally { busy = false }
  }
  async function runNow() {
    busy = true
    error = ''
    try {
      const answer = await agentRequest(tauri, agent.runtimeId, 'agent.run_now', { id: agent.id })
      if (answer?.run) onrun({ ...answer.run, runtimeId: agent.runtimeId })
      await onchange()
    } catch (failure) { error = String(failure?.message ?? failure) }
    finally { busy = false }
  }

  // Tools reads the agent's machine's catalog while it shows, and core's
  // `project_tools` says whether the agent may use each tool and which
  // layer decides: the agent's own choice, its project, or the default.
  let tools = $state({ rows: [], loading: false, error: '' })
  let toolCatalog = $state(null)
  $effect(() => {
    if (panel !== 'tools' || readOnly) return
    let alive = true
    tools = { ...untrack(() => tools), loading: true, error: '' }
    agentRequest(tauri, agent.runtimeId, 'agent.tool_catalog', {})
      .then((answer) => { if (alive) toolCatalog = answer ?? {} })
      .catch((failure) => { if (alive) tools = { rows: [], loading: false, error: String(failure?.message ?? failure) } })
    return () => { alive = false }
  })
  $effect(() => {
    const catalog = toolCatalog
    const choices = agent.tools ?? {}
    const policy = projectTools
    if (!catalog) return
    let alive = true
    Promise.resolve(resolveTools(tauri, catalog, policy, choices))
      .then((rows) => { if (alive) tools = { rows: rows ?? [], loading: false, error: '' } })
      .catch((failure) => { if (alive) tools = { rows: [], loading: false, error: String(failure?.message ?? failure) } })
    return () => { alive = false }
  })
  const toolSource = (row) => row.layer === 'agent' ? 'this agent' : row.layer === 'project' ? (projectName ? `off in project ${projectName}` : 'off in its project') : 'default'
  function chooseTool(row, enabled) {
    void change({ tools: agentToolChoice(agent, projectTools, row, enabled) })
  }

  // Activity reads the newest runs from the agent's machine while it shows,
  // and again when the latest run changes.
  $effect(() => {
    if (panel !== 'activity' || readOnly) return
    latestRun?.id; latestRun?.status
    let alive = true
    runs = { list: untrack(() => runs.list), loading: true, error: '' }
    agentRequest(tauri, agent.runtimeId, 'agent.runs', { agent_id: agent.id, limit: 5 })
      .then((answer) => { if (alive) runs = { list: answer?.runs ?? [], loading: false, error: '' } })
      .catch((failure) => { if (alive) runs = { list: [], loading: false, error: String(failure?.message ?? failure) } })
    return () => { alive = false }
  })

  function pick(id) {
    panel = id
    error = ''
  }
  // Up and down arrows, Home and End move between the panels, like the picker's provider marks.
  function railKeydown(event) {
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
    event.preventDefault()
    const index = PANELS.findIndex((entry) => entry.id === panel)
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? PANELS.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : PANELS.length - 1)) % PANELS.length
    pick(PANELS[next].id)
    const rail = event.currentTarget
    void tick().then(() => rail?.querySelector('[aria-selected="true"]')?.focus())
  }
  function showTip(event, entry) {
    const mark = event.currentTarget
    tip = { label: entry.label, top: mark.offsetTop - mark.parentElement.scrollTop + mark.offsetHeight / 2 }
  }
  const kebabActions = $derived([{ id: 'profile', label: 'Open profile', icon: 'user' }, ...(agent.replica ? [] : [{ id: 'export', label: 'Export', icon: 'download' }])])
</script>

<article class="agent-card" data-agent-card={agent.id} class:read-only={readOnly} class:away={agent.online === false} aria-label={agent.name} style:--agent-color={agentColor(agent)}>
  <header class="card-head">
    <span class="head-avatar"><AgentAvatar {agent} activity={agent.activity} size={28} /></span>
    <div class="head-name">
      <InlineEdit bind:this={nameField} value={agent.name} label="Agent name" maxlength={100} required strong disabled={readOnly} onsave={(name) => saveField({ name })} />
    </div>
    <CatalogActions name={agent.name} {archived} allowRename={false} allowArchive={!agent.replica} allowDelete={!agent.replica} extraActions={kebabActions} onaction={onaction} />
  </header>
  <div class="card-body">
    <div class="panel-rail" role="tablist" aria-orientation="vertical" aria-label={`${agent.name} panels`} tabindex="-1" onkeydown={railKeydown}>
      {#each PANELS as entry (entry.id)}
        {@const chosen = entry.id === panel}
        <button type="button" role="tab" class="rail-mark" id={`agent-${key}-${entry.id}-tab`} aria-label={entry.label} aria-selected={chosen} aria-controls={`agent-${key}-panel`} tabindex={chosen ? 0 : -1}
          onclick={() => pick(entry.id)} onpointerenter={(event) => showTip(event, entry)} onpointerleave={() => tip = null} onfocus={(event) => showTip(event, entry)} onblur={() => tip = null}>
          <LucideIcon name={entry.icon} variant="action" size={16} />
        </button>
      {/each}
    </div>
    {#if tip}<span class="rail-tip" aria-hidden="true" style:top="{tip.top}px">{tip.label}</span>{/if}
    <div class="panel-list" id={`agent-${key}-panel`} role="tabpanel" aria-labelledby={`agent-${key}-${panel}-tab`} data-panel-id={panel} use:panelScroll>
      {#if readOnly}<p class="note" role="note"><LucideIcon name={agent.online === false ? 'globe-off' : 'refresh-cw'} variant="action" size={12} />{readOnlyNote}</p>{/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if panel === 'overview'}
        <h4>Overview</h4>
        <dl class="facts">
          <div class="fact"><dt>Job title</dt><dd><InlineEdit value={agent.label ?? ''} label="Job title" placeholder="Add a job title" disabled={readOnly} onsave={(label) => saveField({ label })} /></dd></div>
          <div class="fact"><dt>Machine</dt><dd class="machine"><MachineMark {machine} />{#if agent.online === false}<StatusChip tone="neutral">Offline</StatusChip>{:else}<StatusChip tone="success">Online</StatusChip>{/if}{#if agent.replica}<span class="synced"><LucideIcon name="refresh-cw" variant="action" size={12} />Synced copy</span>{/if}</dd></div>
          <div class="fact"><dt>Next run</dt><dd class="record">{nextRunLabel(agent)}</dd></div>
          <div class="fact"><dt>Last run</dt><dd class="record">{#if latestRun}<StatusChip tone={runStatusTone(latestRun.status)}>{runStatusLabel(latestRun.status)}</StatusChip><span class="when">{whenLabel(latestRun.updated_at)}</span>{:else}Never{/if}</dd></div>
        </dl>
        <div class="actions">
          <Button icon="play" variant="primary" disabled={readOnly || busy || runActive(latestRun?.status)} onclick={runNow}>Run now</Button>
          <Button icon="messages-square" onclick={() => onchat(agent)}>Open chat</Button>
        </div>
      {:else if panel === 'model'}
        <div class="choices" role="radiogroup" aria-label="Model">
          <h4>Model</h4>
          <button type="button" role="radio" class="row" aria-checked={!agent.model} disabled={readOnly || busy} onclick={() => chooseModel(null)}>
            <span class="row-label">Default model</span>{#if !agent.model}<LucideIcon name="check" variant="action" size={14} />{/if}
          </button>
          {#each groups as group (group.id)}
            <h4 class="provider">{#if group.classifier}<LucideIcon name="route" variant="action" size={12} />{:else}<ProviderLogo color provider={group.providerId} size={12} />{/if}{group.label}</h4>
            {#each group.models as model (`${group.id}:${model.id}`)}
              {@const used = inUse(model)}
              <button type="button" role="radio" class="row" aria-checked={used} data-provider={model.provider} data-model={model.choice} disabled={readOnly || busy} onclick={() => chooseModel(model)}>
                <span class="row-label model-id">{model.label ?? model.id}</span>{#if used}<LucideIcon name="check" variant="action" size={14} />{/if}
              </button>
            {/each}
          {:else}
            <p class="empty">{inventory?.providers?.length ? 'No model is shown.' : 'No provider is connected.'}</p>
          {/each}
        </div>
        <button type="button" class="foot" onclick={onmanagemodels}><LucideIcon name="settings" variant="action" size={14} />Manage models</button>
      {:else if panel === 'permissions'}
        <div class="choices" role="radiogroup" aria-label="Permission mode">
          <h4>Permissions</h4>
          {#if modes === undefined && !readOnly}<p class="empty" role="status">Loading modes…</p>
          {:else if !modes}<p class="empty">{readOnly ? `Modes show once ${machineName} answers.` : `Update ${machineName} to choose a mode.`}</p>{/if}
          {#each modes?.modes ?? [] as mode (mode.id)}
            {@const chosen = mode.id === currentMode?.id}
            {@const line = mode.available === false && mode.reason ? mode.reason : mode.description}
            <!-- One line per mode keeps all six in the card. The chosen mode shows its description. -->
            <button type="button" role="radio" class="row mode" aria-checked={chosen} aria-label={mode.title} aria-describedby={`agent-${key}-mode-${mode.id}`} disabled={readOnly || busy || mode.available === false} onclick={() => chooseMode(mode)}>
              <LucideIcon name={modeIcon(mode)} variant="action" size={16} />
              <span class="mode-text"><span class="mode-title">{mode.title}{#if chosen && !currentMode.chosen}<span class="default">Default</span>{/if}</span><span class="mode-line" class:visually-hidden={!chosen} id={`agent-${key}-mode-${mode.id}`}>{line}</span></span>
              {#if chosen}<LucideIcon name="check" variant="action" size={14} />{/if}
            </button>
          {/each}
        </div>
        <button type="button" class="foot" onclick={() => onprofile(agent)}><LucideIcon name="list-checks" variant="action" size={14} />Rules in the profile</button>
      {:else if panel === 'tools'}
        <h4>Tools</h4>
        {#if readOnly}<p class="empty">Tools show once {machineName} answers.</p>
        {:else if tools.loading && !tools.rows.length}<p class="empty" role="status">Loading tools…</p>
        {:else if tools.error}<p class="error" role="alert">{tools.error}</p>
        {:else}
          <ul class="tools" aria-label="Tools">
            {#each tools.rows as row (`${row.kind}:${row.id}`)}
              <li class="tool">
                <span class="tool-text"><span class="tool-name">{row.label}</span><span class="record">{TOOL_KINDS[row.kind] ?? row.kind} · {toolSource(row)}</span></span>
                <Toggle checked={row.allowed} label={`${row.label} on`} disabled={busy} onchange={(enabled) => chooseTool(row, enabled)} />
              </li>
            {:else}
              <li class="empty">No tools.</li>
            {/each}
          </ul>
        {/if}
      {:else if panel === 'routines'}
        <h4>Routines</h4>
        <ul class="routines" aria-label="Routines">
          {#each routines as routine (routine.id)}
            <li class="routine">
              <button type="button" class="routine-text" aria-label={`Edit ${routine.name} in the profile`} onclick={() => onprofile(agent)}>
                <span class="routine-name">{routine.name}</span>
                <span class="record">{isProposal(routine) ? 'Proposed · ' : ''}{triggerSummary(routine.trigger)}</span>
              </button>
              {#if !isProposal(routine)}<Toggle checked={routine.enabled} label={`${routine.name} on`} disabled={readOnly || busy} onchange={(enabled) => toggleRoutine(routine, enabled)} />{/if}
            </li>
          {:else}
            <li class="empty">No routines.</li>
          {/each}
        </ul>
        {#if agent.paused_reason}<p class="empty">Routines are paused.</p>{/if}
        <button type="button" class="foot" onclick={() => onprofile(agent)}><LucideIcon name="plus" variant="action" size={14} />Add or edit in the profile</button>
      {:else}
        <h4>Activity</h4>
        {#if readOnly}<p class="empty">Runs show once {machineName} answers.</p>
        {:else if runs.loading && !runs.list.length}<p class="empty" role="status">Loading runs…</p>
        {:else if runs.error}<p class="error" role="alert">{runs.error}</p>
        {:else}
          <ul class="runs" aria-label="Recent runs">
            {#each runs.list.slice(0, 5) as run (run.id)}
              <li class="run"><span class="dot" data-tone={runStatusTone(run.status)} aria-hidden="true"></span><span class="run-state">{runStatusLabel(run.status)}</span><span class="record">{triggerLabel(run.trigger?.kind)}{runDuration(run) ? ` · ${runDuration(run)}` : ''}</span><time class="record" datetime={run.updated_at}>{whenLabel(run.updated_at)}</time></li>
            {:else}
              <li class="empty">No runs yet.</li>
            {/each}
          </ul>
        {/if}
      {/if}
    </div>
  </div>
</article>

<style>
  /* The picker's geometry: a hairline under the top row, a 34px mark column with a
     hairline at its right, 6px insets, 30px rows on the hover fill. */
  .agent-card { position: relative; display: flex; flex-direction: column; min-width: 0; height: 100%; overflow: hidden; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  /* The header carries the agent's color: a tinted band with the head on a stronger tile. */
  .card-head { display: grid; grid-template-columns: 46px minmax(0, 1fr) auto; align-items: center; gap: 4px; padding: 6px 8px 6px 0; border-top: 3px solid var(--agent-color); border-bottom: 1px solid color-mix(in srgb, var(--agent-color) 40%, var(--border)); background: color-mix(in oklab, var(--agent-color) 18%, var(--surface)); }
  .head-avatar { display: grid; place-items: center; width: 34px; height: 34px; margin-left: 6px; border-radius: var(--radius-control); background: color-mix(in oklab, var(--agent-color) 34%, var(--surface)); }
  .head-name { min-width: 0; }
  .card-body { position: relative; flex: 1; min-height: 0; display: flex; }
  .panel-rail { flex: none; display: flex; flex-direction: column; gap: 4px; padding: 6px; border-right: 1px solid var(--border); overflow-y: auto; outline: 0; }
  .rail-mark { position: relative; display: grid; place-items: center; width: 34px; height: 34px; padding: 0; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .rail-mark:hover { background: var(--faint); color: var(--ink); }
  .rail-mark[aria-selected="true"] { background: var(--selected); color: var(--on-selected); }
  .rail-mark[aria-selected="true"]::after { content: ''; position: absolute; top: 8px; bottom: 8px; right: -7px; width: 2px; border-radius: var(--radius-chip); background: var(--selected-edge); }
  .rail-tip { position: absolute; z-index: 2; left: 52px; transform: translateY(-50%); padding: 3px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; }
  .panel-list { flex: 1; min-width: 0; min-height: 0; padding: 6px; overflow-y: auto; }
  h4 { display: flex; align-items: center; gap: 6px; margin: 6px 8px 2px; color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  h4.provider { margin-top: 10px; }
  .facts { display: grid; margin: 0; }
  .fact { display: grid; grid-template-columns: 72px minmax(0, 1fr); align-items: center; gap: 8px; min-height: 30px; padding: 0 0 0 8px; }
  dt { color: var(--muted); font-size: var(--text-12); }
  dd { min-width: 0; margin: 0; }
  dd.record, .fact .machine { padding: 0 6px; }
  .machine { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; }
  .synced { display: inline-flex; align-items: center; gap: 4px; color: var(--muted); font: var(--text-12) var(--font-human); }
  .record { min-width: 0; overflow: hidden; color: var(--muted); font: var(--text-12) var(--font-human); text-overflow: ellipsis; white-space: nowrap; }
  dd.record:has(.when) { display: flex; align-items: center; gap: 8px; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; padding: 10px 8px 4px; }
  .choices { display: grid; }
  .row { display: flex; width: 100%; align-items: center; justify-content: space-between; gap: 8px; min-height: 30px; padding: 5px 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .row:hover:not(:disabled) { background: var(--faint); }
  .row[aria-checked="true"] { background: var(--selected); color: var(--on-selected); }
  .row:disabled { cursor: default; }
  .row:disabled:not([aria-checked="true"]) { color: var(--muted); }
  .row-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .model-id { font: var(--text-13) var(--font-human); }
  .mode { justify-content: flex-start; align-items: flex-start; }
  .mode > :global(svg:first-child) { flex: none; margin-top: 2px; }
  .mode > :global(svg:last-child:not(:first-child)) { flex: none; margin: 2px 0 0 auto; }
  .mode-text { display: grid; gap: 2px; min-width: 0; }
  .mode-title { display: flex; align-items: baseline; gap: 6px; font-size: var(--text-13); }
  .default { color: var(--muted); font: var(--text-12) var(--font-human); }
  .mode-line { color: var(--muted); font-size: var(--text-12); line-height: 1.4; }
  .routines, .runs, .tools { display: grid; margin: 0; padding: 0; list-style: none; }
  .tool { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 30px; padding: 2px 4px 2px 8px; }
  .tool-text { display: grid; gap: 1px; min-width: 0; }
  .tool-name { overflow: hidden; font-size: var(--text-13); text-overflow: ellipsis; white-space: nowrap; }
  .routine { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 30px; padding: 2px 4px 2px 0; }
  .routine-text { display: grid; gap: 2px; min-width: 0; padding: 4px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: inherit; text-align: left; cursor: pointer; }
  .routine-text:hover { background: var(--faint); }
  .routine-name { overflow: hidden; font-size: var(--text-13); font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .run { display: grid; grid-template-columns: auto auto minmax(0, 1fr) auto; align-items: center; gap: 8px; min-height: 30px; padding: 0 8px; }
  .run-state { font-size: var(--text-13); }
  .dot { width: 6px; height: 6px; border-radius: var(--radius-chip); background: var(--muted); }
  .dot[data-tone="success"] { background: var(--success); }
  .dot[data-tone="active"] { background: var(--active); }
  .dot[data-tone="danger"] { background: var(--danger); }
  .dot[data-tone="warning"] { background: var(--warning); }
  .empty { margin: 0; padding: 6px 8px; color: var(--muted); font-size: var(--text-12); }
  .note { display: flex; align-items: flex-start; gap: 6px; margin: 2px 2px 6px; padding: 6px 8px; border-radius: var(--radius-control); background: var(--faint); color: var(--ink); font-size: var(--text-12); line-height: 1.4; }
  .note > :global(svg) { flex: none; margin-top: 2px; color: var(--ochre); }
  .read-only:not(.away) .note > :global(svg) { color: var(--muted); }
  .error { margin: 2px 8px 6px; color: var(--oxide); font-size: var(--text-12); line-height: 1.4; overflow-wrap: anywhere; }
  /* The picker's foot action, under a separator inset to the list's padding. */
  .foot { position: relative; display: flex; align-items: center; gap: 8px; width: 100%; min-height: 30px; margin-top: 6px; padding: 7px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .foot::before { content: ''; position: absolute; top: -3px; left: 0; right: 0; height: 1px; background: var(--border); opacity: .75; }
  .foot:hover { background: var(--faint); }
</style>
