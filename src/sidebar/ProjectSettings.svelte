<script>
  // A project's settings: what a new thread in it starts with, each with
  // where its value comes from, the instructions every run in its folder
  // reads, the tools its runs may use, and its agents. An agent is
  // the project's triggers: each agent's card holds its model, permissions,
  // routines and activity, and Add a trigger makes the project an agent with
  // a home thread where its runs post. Core saves the defaults and the
  // instructions through `project_settings_save`, and each agent change is an
  // agent operation on the agent's own machine.
  import PanelPage from '../lib/ui/PanelPage.svelte'
  import PageHead from '../lib/ui/PageHead.svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Button from '../lib/ui/Button.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import AgentCard from '../agents/AgentCard.svelte'
  import { agentMachine, agentRequest } from '../agents/agent-runtime.js'
  import { loadModes } from '../agents/permission-modes.js'
  import { TOOL_KINDS, projectToolPolicy, projectTools } from '../agents/agent-tools.js'
  import { pickerGroups } from '../lib/provider-catalog.js'
  import { machineName } from '../runtimes/runtimes.js'
  import { sourceLine } from './sources.js'
  import { untrack } from 'svelte'

  let { tauri, project, agents = [], inventory = null, modes = null, runtimeList = null, latestRuns = {}, busy = false,
    onback, onnewthread, onaddtrigger, onchat = () => {}, onprofile = () => {}, onaction = async () => {}, onchange = async () => {}, onrun = () => {}, onmanagemodels = () => {} } = $props()

  let settings = $state(null)
  let instructions = $state('')
  let error = $state('')
  let saving = $state(false)
  let saved = $state('')
  let adding = $state(false)
  const chats = $derived(project.kind === 'chats')
  const plain = $derived(project.kind === 'project')
  const remote = $derived(!!project.owner)

  $effect(() => {
    const id = project.id
    untrack(() => {
      settings = null; error = ''; saved = ''
      if (project.kind === 'agent') return
      tauri.invoke('project_settings', { projectId: id })
        .then((value) => { if (project.id === id) { settings = value ?? {}; instructions = value?.instructions ?? '' } })
        .catch((reason) => { if (project.id === id) error = String(reason?.message ?? reason) })
    })
  })

  const SEP = '\u0000'
  const modelGroups = $derived(pickerGroups(inventory).map((group) => ({ ...group, models: group.models.filter((model) => model.shown !== false && !model.off) })).filter((group) => group.models.length))
  const modelValue = $derived(settings?.model ? `${settings.model.provider}${SEP}${settings.model.model}` : '')
  const machines = $derived((runtimeList?.runtimes ?? []).filter((runtime) => !runtime.local))
  // Each default's value and where it comes from: this project, or the
  // machine's default when the project names none.
  const modelLabel = (provider, model) => modelGroups.flatMap((group) => group.models).find((entry) => entry.provider === provider && entry.choice === model)?.label ?? model
  const modelSource = $derived(settings?.model
    ? `${modelLabel(settings.model.provider, settings.model.model)} · this project`
    : sourceLine(inventory?.default_model ? modelLabel(inventory.default_provider, inventory.default_model) : '', 'default'))
  const modeTitle = (id) => modes?.modes?.find((mode) => mode.id === id)?.title ?? id
  const modeSource = $derived(settings?.permissionMode
    ? `${modeTitle(settings.permissionMode)} · this project`
    : sourceLine(modes?.default_thread_mode ? modeTitle(modes.default_thread_mode) : '', 'default'))
  const machineSource = $derived(settings?.machine
    ? `${machineName(machines.find((machine) => machine.id === settings.machine) ?? { id: settings.machine })} · this project`
    : sourceLine('This computer', 'default'))

  // The tools this computer offers and whether the project's runs may use
  // each, from core's `project_tools`. A tool stays allowed until the
  // project turns it off.
  let toolCatalog = $state(null)
  let toolRows = $state([])
  $effect(() => {
    void project.id
    if (project.kind === 'agent' || remote) return
    let alive = true
    agentRequest(tauri, 'local', 'agent.tool_catalog', {})
      .then((answer) => { if (alive) toolCatalog = answer ?? {} })
      .catch(() => { if (alive) toolCatalog = {} })
    return () => { alive = false }
  })
  $effect(() => {
    const catalog = toolCatalog
    const policy = settings?.tools ?? null
    if (!catalog || !settings) { toolRows = []; return }
    let alive = true
    Promise.resolve(projectTools(tauri, catalog, policy)).then((rows) => { if (alive) toolRows = rows ?? [] }).catch(() => { if (alive) toolRows = [] })
    return () => { alive = false }
  })
  function toggleTool(row, enabled) {
    return save({ tools: projectToolPolicy(settings?.tools, row, enabled) })
  }

  async function save(patch) {
    if (!settings) return
    const next = { ...settings, ...patch }
    saving = true; error = ''; saved = ''
    try {
      await tauri.invoke('project_settings_save', { projectId: project.id, settings: next })
      settings = next
      saved = 'Saved'
    } catch (reason) {
      error = String(reason?.message ?? reason)
    } finally { saving = false }
  }
  function chooseModel(value) {
    if (!value) return save({ model: null })
    const [provider, model] = value.split(SEP)
    return save({ model: { provider, model } })
  }
  async function addTrigger() {
    adding = true; error = ''
    try { await onaddtrigger(project) } catch (reason) { error = String(reason?.message ?? reason) } finally { adding = false }
  }

  // Each agent machine's permission modes, read once per machine.
  let agentModes = $state({})
  $effect(() => {
    const runtimes = agents.filter((agent) => !agent.replica && agent.online !== false).map((agent) => agent.runtimeId ?? 'local')
    untrack(() => {
      for (const runtime of new Set(runtimes)) {
        if (runtime in agentModes) continue
        agentModes[runtime] = undefined
        void loadModes(tauri, runtime).then((value) => { agentModes[runtime] = value })
      }
    })
  })
</script>

<section data-panel="projects" class="panel" aria-label="Project settings">
  <PanelPage>
    <PageHead title={project.name} level={2} intro={chats ? 'Chats holds quick chats that belong to no other project.' : 'A project shares its instructions, memory and tools with every thread in it.'}>
      {#snippet lead()}<button type="button" class="back" aria-label="Back to the thread" onclick={onback}><LucideIcon name="arrow-left" /></button>{/snippet}
      {#snippet actions()}
        {#if saved}<span class="saved" role="status">{saved}</span>{/if}
        <Button icon="square-pen" disabled={busy} onclick={() => onnewthread(project.id)}>New thread</Button>
      {/snippet}
    </PageHead>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if project.kind !== 'agent'}
      {#if !settings && !error}<p class="note" role="status">Loading settings…</p>{/if}
      {#if settings}
        <Group heading="New threads" info="A new thread in this project starts with this model, permission mode and machine.">
          <Row title="Model" description={modelSource}>
            <select aria-label="Default model" disabled={saving} value={modelValue} onchange={(event) => chooseModel(event.currentTarget.value)}>
              <option value="">Default model</option>
              {#each modelGroups as group (group.id)}<optgroup label={group.name}>{#each group.models as model (`${model.provider}:${model.choice}`)}<option value={`${model.provider}${SEP}${model.choice}`}>{model.label}</option>{/each}</optgroup>{/each}
            </select>
          </Row>
          <Row title="Permission mode" description={modeSource}>
            <select aria-label="Default permission mode" disabled={saving || !modes} value={settings.permissionMode ?? ''} onchange={(event) => save({ permissionMode: event.currentTarget.value || null })}>
              <option value="">The default mode</option>
              {#each modes?.modes ?? [] as mode (mode.id)}<option value={mode.id} disabled={mode.available === false}>{mode.title ?? mode.label ?? mode.id}</option>{/each}
            </select>
          </Row>
          {#if machines.length}
            <Row title="Machine" description={machineSource}>
              <select aria-label="Default machine" disabled={saving} value={settings.machine ?? ''} onchange={(event) => save({ machine: event.currentTarget.value || null })}>
                <option value="">This computer</option>
                {#each machines as machine (machine.id)}<option value={machine.id}>{machineName(machine)}</option>{/each}
              </select>
            </Row>
          {/if}
        </Group>
        {#if plain && !remote}
          <Group heading="Instructions" info="Every run in this project reads them from AGENTS.md in its folder.">
            <Row stacked title="" label="Project instructions">
              <textarea aria-label="Project instructions" rows="6" placeholder="How replies in this project should work" disabled={saving} bind:value={instructions}></textarea>
              <div class="actions"><Button variant="primary" disabled={saving || instructions === (settings.instructions ?? '')} onclick={() => save({ instructions })}>Save instructions</Button></div>
            </Row>
          </Group>
        {/if}
        {#if !remote && toolRows.length}
          <Group heading="Tools" info="Every run in this project, and each of its agents unless it chooses otherwise, may use the tools that are on.">
            <ul class="tool-grid" aria-label="Project tools">
              {#each toolRows as row (`${row.kind}:${row.id}`)}
                <li class="tool">
                  <span class="tool-text"><span class="tool-name">{row.label}</span><span class="tool-source">{TOOL_KINDS[row.kind] ?? row.kind} · {row.layer === 'project' ? 'off in this project' : 'default'}</span></span>
                  <Toggle checked={row.allowed} disabled={saving} aria-label={`Allow ${row.label} in this project`} onchange={(checked) => toggleTool(row, checked)} />
                </li>
              {/each}
            </ul>
          </Group>
        {/if}
      {/if}
    {/if}
    {#if !chats}
      <Group heading="Triggers" info="A schedule, a webhook or a tool check starts a run. Runs post to the project's home thread.">
        {#snippet actions()}{#if plain && !remote}<Button icon="plus" disabled={busy || adding} onclick={addTrigger}>Add a trigger</Button>{/if}{/snippet}
        {#if !agents.length}<Row title="No triggers" description="Threads here start only when you write." />{/if}
      </Group>
      {#if agents.length}
        <div class="cards" aria-label="Project agents">
          {#each agents as agent (`${agent.runtimeId ?? 'local'}:${agent.id}`)}
            <AgentCard {agent} {tauri} {inventory} projectTools={settings?.tools ?? null} projectName={project.name} modes={agent.replica || agent.online === false ? null : agentModes[agent.runtimeId ?? 'local']} machine={agent.primaryMachine ?? agentMachine(runtimeList, agent)} latestRun={latestRuns[agent.id] ?? null}
              {onchat} {onprofile} {onrun} {onmanagemodels} {onchange} onaction={(action, name) => onaction(agent, action, name)} />
          {/each}
        </div>
      {/if}
    {/if}
  </PanelPage>
</section>

<style>
  .panel { grid-area: thread; display: flex; flex-direction: column; overflow: hidden; }
  .back { display: inline-flex; align-items: center; justify-content: center; flex: none; width: 32px; height: 32px; padding: 0; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .back:hover { background: var(--faint); color: var(--ink); }
  select { min-width: 180px; max-width: 260px; height: 30px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  textarea { width: 100%; box-sizing: border-box; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); resize: vertical; }
  textarea:focus, select:focus { outline: none; border-color: var(--muted); }
  .actions { display: flex; justify-content: flex-end; margin-top: 8px; }
  .tool-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(240px, 1fr)); gap: 0 24px; margin: 0; padding: 6px 0; list-style: none; }
  .tool { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-width: 0; min-height: 40px; }
  .tool-text { display: grid; gap: 1px; min-width: 0; }
  .tool-name { overflow: hidden; font-size: var(--text-13); font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .tool-source { overflow: hidden; color: var(--muted); font-size: var(--text-12); text-overflow: ellipsis; white-space: nowrap; }
  .cards { display: grid; grid-template-columns: repeat(auto-fill, minmax(340px, 1fr)); grid-auto-rows: 300px; gap: 12px; }
  .note, .error, .saved { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .error { color: var(--danger); }
</style>
