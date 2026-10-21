<script>
  // One agent on the Agents page, laid out like the composer's model picker:
  // the avatar and the name across the top, a rail of panel icons down the
  // left, and the picked panel on the right. Each panel holds the settings
  // that edit in one step, drawn as the profile draws them; the rest stay in
  // the profile. Every change is an agent operation on the agent's own
  // machine, and core validates it. A synced copy, or an agent whose machine
  // does not answer, is read only.
  import { kindIcon, machineName as machineLabel } from '../runtimes/runtimes.js'
  import { tick, untrack } from 'svelte'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import { agentColor } from '../lib/agent-avatar.js'
  import CatalogActions from '../lib/CatalogActions.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import ModelPicker from '../lib/ModelPicker.svelte'
  import Button from '../lib/ui/Button.svelte'
  import ComposerMenuButton from '../composer/ComposerMenuButton.svelte'
  import ModePicker from '../composer/ModePicker.svelte'
  import { panelScroll } from '../lib/panel-scroll.js'
  import { runActive, runLight, runStatusLabel, whenLabel } from '../lib/agent-labels.js'
  import { agentRequest, saveAgentPatch } from './agent-runtime.js'
  import InlineEdit from './InlineEdit.svelte'
  import RoutineRow from './RoutineRow.svelte'
  import RunHistory, { runIcon } from './RunHistory.svelte'
  import { agentMode, modeIcon, modePermissions } from './permission-modes.js'
  import { isProposal, nextRunLabel } from './routine-labels.js'

  // `modes` is core's `permission.modes` from the agent's machine: undefined
  // while it loads, null when the machine lists none. `onopenrun` opens a
  // run's conversation, and `onvisibility` moves a model between the picker's lists.
  let { agent, tauri, machine, inventory = null, latestRun = null, modes = undefined, archived = false,
    onchat = () => {}, onprofile = () => {}, onaction = async () => {}, onchange = async () => {}, onrun = () => {}, onopenrun = null, onvisibility = null, onmanagemodels = () => {} } = $props()

  const PANELS = [
    { id: 'overview', label: 'Overview', icon: 'layout-dashboard' },
    { id: 'model', label: 'Model', icon: 'cpu' },
    { id: 'permissions', label: 'Permissions', icon: 'shield-check' },
    { id: 'routines', label: 'Routines', icon: 'clock' },
    { id: 'history', label: 'History', icon: 'history' },
  ]
  let panel = $state('overview')
  let tip = $state(null)
  let busy = $state(false)
  let error = $state('')
  let runs = $state({ list: [], loading: false, error: '' })
  let nameField = $state()
  let modeOpen = $state(false)
  let modeButton = $state()

  const key = $derived(`${agent.runtimeId ?? 'local'}-${agent.id}`)
  const readOnly = $derived(!!agent.replica || agent.online === false)
  const machineName = $derived(machine ? machineLabel(machine) : 'its machine')
  const readOnlyNote = $derived(agent.online === false ? `Read only while ${machineName} is offline.` : `Synced copy. Edit it on ${machineName}.`)
  const machineState = $derived(agent.online === false ? 'Offline' : 'Online')
  const currentMode = $derived(agentMode(agent, modes))
  const routines = $derived((agent.routines ?? []).filter((routine) => !isProposal(routine)))
  const proposed = $derived((agent.routines ?? []).filter(isProposal).length)

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
  function chooseModel(provider, choice) {
    if (agent.model?.provider === provider && agent.model?.id === choice) return
    void change({ model: { provider, id: choice, ...(agent.model?.thinking ? { thinking: agent.model.thinking } : {}) } })
  }
  async function closeMode() {
    modeOpen = false
    await tick()
    modeButton?.focus()
  }
  function chooseMode(id) {
    void closeMode()
    const mode = modes?.modes?.find((entry) => entry.id === id)
    if (!mode || (id === currentMode?.id && currentMode.chosen) || mode.available === false) return
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

  // History reads the newest runs from the agent's machine while it shows,
  // and again when the latest run changes.
  $effect(() => {
    if (panel !== 'history' || readOnly) return
    latestRun?.id; latestRun?.status
    let alive = true
    runs = { list: untrack(() => runs.list), loading: true, error: '' }
    agentRequest(tauri, agent.runtimeId, 'agent.runs', { agent_id: agent.id, limit: 20 })
      .then((answer) => { if (alive) runs = { list: (answer?.runs ?? []).map((run) => ({ ...run, runtimeId: agent.runtimeId ?? 'local' })), loading: false, error: '' } })
      .catch((failure) => { if (alive) runs = { list: [], loading: false, error: String(failure?.message ?? failure) } })
    return () => { alive = false }
  })

  // A press outside the open mode menu and its button closes it, and so does
  // a scroll or a resize, since the menu sits on the page under its button.
  $effect(() => {
    if (!modeOpen) return
    const outside = (event) => { if (!event.target?.closest?.(`[data-card-menu="${key}"]`)) modeOpen = false }
    const moved = (event) => { if (!event.target?.closest?.(`.card-menu[data-card-menu="${key}"]`)) modeOpen = false }
    window.addEventListener('pointerdown', outside, true)
    window.addEventListener('scroll', moved, true)
    window.addEventListener('resize', moved)
    return () => { window.removeEventListener('pointerdown', outside, true); window.removeEventListener('scroll', moved, true); window.removeEventListener('resize', moved) }
  })
  // The mode menu opens on the page, under its button or above it when the
  // window has more room there, since the card clips what overflows it.
  function underButton(node) {
    document.body.append(node)
    const anchor = modeButton.getBoundingClientRect()
    const below = window.innerHeight - anchor.bottom - 12
    const above = anchor.top - 12
    const left = Math.max(8, Math.min(anchor.left, window.innerWidth - Math.min(360, window.innerWidth - 16) - 8))
    Object.assign(node.style, below >= Math.min(node.offsetHeight, 320) || below >= above
      ? { top: `${anchor.bottom + 4}px`, left: `${left}px`, maxHeight: `${below}px` }
      : { bottom: `${window.innerHeight - anchor.top + 4}px`, left: `${left}px`, maxHeight: `${above}px` })
    return { destroy() { node.remove() } }
  }

  function pick(id) {
    panel = id
    error = ''
    modeOpen = false
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
  // A rail mark's name shows outside the card, left of its rail, so it never
  // covers a panel's labels. The tip sits on the page, since the card clips
  // what overflows it.
  let card = $state()
  function showTip(event, entry) {
    const mark = event.currentTarget.getBoundingClientRect()
    tip = { label: entry.label, top: mark.top + mark.height / 2, right: window.innerWidth - card.getBoundingClientRect().left + 6 }
  }
  // A card at the window's left edge shows the tip beside its right edge.
  function onPage(node) {
    document.body.append(node)
    if (node.getBoundingClientRect().left < 4) Object.assign(node.style, { right: 'auto', left: `${card.getBoundingClientRect().right + 6}px` })
    return { destroy() { node.remove() } }
  }
  const kebabActions = $derived([{ id: 'profile', label: 'Open profile', icon: 'user' }, ...(agent.replica ? [] : [{ id: 'export', label: 'Export', icon: 'download' }])])
</script>

<article class="agent-card" bind:this={card} data-agent-card={agent.id} class:read-only={readOnly} class:away={agent.online === false} aria-label={agent.name} style:--agent-color={agentColor(agent)}>
  <header class="card-head">
    <span class="head-avatar"><AgentAvatar {agent} activity={agent.activity} size={28} /></span>
    <div class="head-name">
      <InlineEdit bind:this={nameField} value={agent.name} label="Agent name" maxlength={100} required strong disabled={readOnly} onsave={(name) => saveField({ name })} />
    </div>
    <CatalogActions name={agent.name} {archived} allowRename={false} allowArchive={!agent.replica} allowDelete={!agent.replica} confirmDelete={false} extraActions={kebabActions} onaction={onaction} />
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
    {#if tip}<span class="rail-tip" aria-hidden="true" use:onPage style:top="{tip.top}px" style:right="{tip.right}px">{tip.label}</span>{/if}
    <div class="panel-list" class:fill={panel === 'model'} id={`agent-${key}-panel`} role="tabpanel" aria-labelledby={`agent-${key}-${panel}-tab`} data-panel-id={panel} use:panelScroll>
      {#if readOnly}<p class="note" role="note"><LucideIcon name={agent.online === false ? 'globe-off' : 'refresh-cw'} variant="action" size={12} />{readOnlyNote}</p>{/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      {#if panel === 'overview'}
        <h4>Overview</h4>
        <!-- Each state reads in words at the left and shows its stoplight signal at the row's end. -->
        <dl class="facts">
          <div class="fact"><dt>Job title</dt><dd><InlineEdit value={agent.label ?? ''} label="Job title" placeholder="Add a job title" disabled={readOnly} onsave={(label) => saveField({ label })} /></dd></div>
          <div class="fact"><dt>Machine</dt><dd class="record signaled"><span class="words">{machine ? machineName : ''}{#if agent.replica}<span class="synced"><LucideIcon name="refresh-cw" variant="action" size={12} />Synced copy</span>{/if}</span><span class="signal" data-light={agent.online === false ? 'red' : 'green'} role="img" aria-label={machineState} data-tooltip={machineState}><LucideIcon name={kindIcon(machine)} variant="action" size={14} /></span></dd></div>
          <div class="fact"><dt>Next run</dt><dd class="record">{nextRunLabel(agent)}</dd></div>
          <div class="fact"><dt>Last run</dt><dd class="record signaled">{#if latestRun}{@const state = runStatusLabel(latestRun.status)}<span class="words">{whenLabel(latestRun.updated_at)}</span><span class="signal" data-light={runLight(latestRun)} role="img" aria-label={state} data-tooltip={state}><LucideIcon name={runIcon(latestRun)} variant="action" size={14} /></span>{:else}<span class="words">Never</span>{/if}</dd></div>
        </dl>
        <div class="actions">
          <Button icon="play" variant="primary" disabled={readOnly || busy || runActive(latestRun?.status)} onclick={runNow}>Run now</Button>
          <Button icon="messages-square" onclick={() => onchat(agent)}>Open chat</Button>
        </div>
      {:else if panel === 'model'}
        <ModelPicker embedded {inventory} current={agent.model ? { provider: agent.model.provider, model: agent.model.id } : null} source="this agent" actions={agent.model ? [{ id: 'default', label: 'Use the default model' }] : []} disabled={readOnly || busy}
          onaction={() => change({ model: null })} onchoose={chooseModel} {onvisibility} onmanage={onmanagemodels} onrouting={onmanagemodels} />
      {:else if panel === 'permissions'}
        <h4>Permissions</h4>
        {#if modes === undefined && !readOnly}<p class="empty" role="status">Loading modes…</p>
        {:else if !modes}<p class="empty">{readOnly ? `Modes show once ${machineName} answers.` : `Update ${machineName} to choose a mode.`}</p>
        {:else}
          <div class="menu-row" data-card-menu={key}>
            <ComposerMenuButton menu="agent-mode" bind:element={modeButton} open={modeOpen} chevrons label={`Permissions: ${currentMode?.title ?? 'Choose'}`} text={currentMode?.title ?? 'Choose'} disabled={readOnly || busy} onclick={() => { modeOpen = !modeOpen }}>{#snippet icon()}<LucideIcon name={modeIcon(currentMode)} size={14} />{/snippet}</ComposerMenuButton>
          </div>
          {#if modeOpen}<div class="card-menu" data-card-menu={key} use:underButton><ModePicker modes={modes.modes} current={currentMode?.id} source={currentMode?.chosen ? '' : 'default'} hint={false} onchoose={chooseMode} onclose={closeMode} /></div>{/if}
        {/if}
      {:else if panel === 'routines'}
        <h4>Routines</h4>
        <div class="routines" aria-label="Routines" role="group">
          {#each routines as routine (routine.id)}
            <RoutineRow {routine} disabled={readOnly || busy} editLabel={`Edit ${routine.name} in the profile`} onedit={() => onprofile(agent)} ontoggle={toggleRoutine} />
          {:else}
            <p class="empty">No routines. It runs only when you ask.</p>
          {/each}
        </div>
        {#if proposed}<p class="empty">{proposed === 1 ? 'One proposed routine waits in the profile.' : `${proposed} proposed routines wait in the profile.`}</p>{/if}
        {#if agent.paused_reason}<p class="empty">Routines are paused.</p>{/if}
        <button type="button" class="foot" onclick={() => onprofile(agent)}><LucideIcon name="plus" variant="action" size={14} />Add or edit in the profile</button>
      {:else}
        <h4>History</h4>
        {#if readOnly}<p class="empty">Runs show once {machineName} answers.</p>
        {:else if runs.loading && !runs.list.length}<p class="empty" role="status">Loading runs…</p>
        {:else if runs.error}<p class="error" role="alert">{runs.error}</p>
        {:else}<RunHistory embedded {tauri} {agent} runs={runs.list} disabled={busy} {onrun} onopen={onopenrun} />{/if}
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
  .rail-tip { position: fixed; z-index: 30; transform: translateY(-50%); padding: 3px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; }
  .panel-list { flex: 1; min-width: 0; min-height: 0; padding: 6px; overflow-y: auto; }
  /* The model picker fills the panel and scrolls its own list. */
  .panel-list.fill { display: flex; flex-direction: column; padding: 0; overflow: hidden; }
  .fill > :global(.model-picker) { flex: 1; height: auto; min-height: 0; }
  .fill > .note { margin: 6px 6px 0; }
  .fill > .error { margin: 6px 8px 0; }
  h4 { display: flex; align-items: center; gap: 6px; margin: 6px 8px 2px; color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  .facts { display: grid; margin: 0; }
  .fact { display: grid; grid-template-columns: 72px minmax(0, 1fr); align-items: center; gap: 8px; min-height: 30px; padding: 0 0 0 8px; }
  dt { color: var(--muted); font-size: var(--text-12); }
  dd { min-width: 0; margin: 0; }
  dd.record { padding: 0 6px; }
  .synced { display: inline-flex; align-items: center; gap: 4px; margin-left: 8px; color: var(--muted); font: var(--text-12) var(--font-human); }
  .record { min-width: 0; overflow: hidden; color: var(--muted); font: var(--text-12) var(--font-human); text-overflow: ellipsis; white-space: nowrap; }
  /* A state row: its words at the left, its signal in one column at the row's end, in the history's stoplight colors. */
  .signaled { display: flex; align-items: center; gap: 8px; }
  .words { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .signal { position: relative; flex: none; display: inline-flex; color: var(--warning); }
  .signal::after { content: attr(data-tooltip); position: absolute; z-index: 2; right: -4px; bottom: calc(100% + 6px); padding: 3px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); white-space: nowrap; pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  .signal:hover::after { opacity: 1; transition-delay: 350ms; }
  .signal[data-light="green"] { color: var(--success); }
  .signal[data-light="red"] { color: var(--danger); }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; padding: 10px 8px 4px; }
  .menu-row { padding: 2px 4px; }
  /* The mode menu sits on the page under its button, as the profile's menu opens under its row. */
  .card-menu { position: fixed; z-index: 30; display: flex; width: min(360px, calc(100vw - 16px)); }
  .card-menu > :global([data-panel-variant="overlay"]) { position: static; flex: 1; max-height: inherit; }
  .routines { display: grid; gap: 6px; padding: 4px 8px; }
  .empty { margin: 0; padding: 6px 8px; color: var(--muted); font-size: var(--text-12); }
  .note { display: flex; align-items: flex-start; gap: 6px; margin: 2px 2px 6px; padding: 6px 8px; border-radius: var(--radius-control); background: var(--faint); color: var(--ink); font-size: var(--text-12); line-height: 1.4; }
  .note > :global(svg) { flex: none; margin-top: 2px; color: var(--warning); }
  .read-only:not(.away) .note > :global(svg) { color: var(--muted); }
  .error { margin: 2px 8px 6px; color: var(--danger); font-size: var(--text-12); line-height: 1.4; overflow-wrap: anywhere; }
  /* The picker's foot action, under a separator inset to the list's padding. */
  .foot { position: relative; display: flex; align-items: center; gap: 8px; width: 100%; min-height: 30px; margin-top: 6px; padding: 7px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .foot::before { content: ''; position: absolute; top: -3px; left: 0; right: 0; height: 1px; background: var(--border); opacity: .75; }
  .foot:hover { background: var(--faint); }
</style>
