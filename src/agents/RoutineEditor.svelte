<script>
  // Adds or edits one routine: its name, trigger and prompt. The runtime
  // validates and stores the routine, and its answer or refusal shows here.
  import { onMount, tick } from 'svelte'
  import { dialogDismiss } from '../lib/dialog-dismiss.js'
  import PopupClose from '../lib/PopupClose.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import Button from '../lib/ui/Button.svelte'
  import CronEditor from './CronEditor.svelte'
  import ConditionBuilder from './ConditionBuilder.svelte'
  import WebhookPanel from './WebhookPanel.svelte'
  import { conditionToTree, treeToCondition } from './condition.js'
  import { localTimeZone } from './cron.js'
  import { serverTools } from './agent-tools.js'

  // A new webhook routine goes through `oncreatewebhook`, which answers core's
  // `routine.webhook_create`: the routine, its address and its one-time secret.
  // `catalog` is the agent machine's `agent.tool_catalog`, whose MCP servers
  // and their tools fill the pickers. Both fields still take any name.
  let { routine = null, catalog = null, webhooks = false, tauri = null, agent = null, onsave, oncreatewebhook = null, ondelete = null, onclose } = $props()
  const original = routine
  const trigger = original?.trigger ?? { kind: 'cron', expr: '0 9 * * *', tz: localTimeZone() }
  let name = $state(original?.name ?? '')
  let enabled = $state(original?.enabled ?? true)
  let prompt = $state(original?.prompt ?? '')
  let kind = $state(trigger.kind)
  let cron = $state(trigger.kind === 'cron' ? { expr: trigger.expr, tz: trigger.tz } : { expr: '0 9 * * *', tz: localTimeZone() })
  let preset = $state(trigger.kind === 'webhook' ? trigger.preset ?? 'generic' : 'generic')
  let server = $state(trigger.kind === 'mcp' ? trigger.server : '')
  let tool = $state(trigger.kind === 'mcp' ? trigger.tool : '')
  let args = $state(trigger.kind === 'mcp' ? JSON.stringify(trigger.args ?? {}, null, 2) : '{}')
  let every = $state(trigger.kind === 'mcp' ? { ...trigger.every } : { expr: '*/15 * * * *', tz: localTimeZone() })
  let condition = $state(conditionToTree(trigger.kind === 'mcp' ? trigger.condition : null))
  let busy = $state(false)
  let error = $state('')
  const offered = $derived(serverTools(catalog, server.trim()))
  let confirmDelete = $state(false)
  let created = $state(null)
  let panel
  const id = $props.id()

  onMount(() => { void tick().then(() => panel?.querySelector('input')?.focus()) })

  function buildTrigger() {
    if (kind === 'cron') return { kind, expr: cron.expr.trim(), tz: cron.tz.trim() }
    if (kind === 'webhook') return { ...(trigger.kind === 'webhook' ? trigger : {}), kind, preset }
    let parsed
    try { parsed = JSON.parse(args.trim() || '{}') } catch { throw new Error('Enter the tool arguments as JSON.') }
    return { kind, server: server.trim(), tool: tool.trim(), args: parsed, every: { expr: every.expr.trim(), tz: every.tz.trim() }, condition: treeToCondition(condition) }
  }

  async function save(event) {
    event.preventDefault()
    busy = true
    error = ''
    try {
      if (kind === 'webhook' && !original && oncreatewebhook) {
        created = await oncreatewebhook({ name: name.trim(), enabled, prompt, trigger: { kind: 'webhook', preset } })
        return
      }
      const next = { ...(original ?? { id: '', created_from: 'user', last_fired_at: null, last_result: null }), name: name.trim(), enabled, prompt, trigger: buildTrigger() }
      await onsave(next)
      onclose()
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = false
    }
  }

  async function remove() {
    busy = true
    error = ''
    try { await ondelete(original); onclose() } catch (failure) { error = String(failure?.message ?? failure) } finally { busy = false }
  }

  // The dialog mounts on the page body, so the profile's scrolling panel
  // cannot clip it.
  function portal(node) {
    document.body.appendChild(node)
    return { destroy() { node.remove() } }
  }

  function keydown(event) {
    if (event.key === 'Escape' && !busy) { event.preventDefault(); event.stopPropagation(); onclose() }
  }
</script>

<svelte:window onkeydown={keydown} />

<div class="dialog-backdrop" use:portal use:dialogDismiss={{ onclose, disabled: busy }}>
  <div data-panel="routine" data-panel-variant="overlay" class="dialog-panel" role="dialog" aria-modal="true" aria-labelledby={`${id}-title`} bind:this={panel}>
    <header><h2 id={`${id}-title`}>{original ? 'Edit routine' : 'New routine'}</h2><PopupClose label="Close routine" disabled={busy} onclick={onclose} /></header>
    <form onsubmit={save}>
      <div class="body">
        <div class="line">
          <label class="grow">Name<input bind:value={name} maxlength="80" required disabled={busy} /></label>
          <label class="switch"><Toggle bind:checked={enabled} label="Routine on" disabled={busy} />On</label>
        </div>
        <label>Starts on<select bind:value={kind} disabled={busy || (original && original.trigger.kind === 'webhook')}>
          <option value="cron">A schedule</option>
          <option value="webhook" disabled={!webhooks && trigger.kind !== 'webhook'}>A webhook</option>
          <option value="mcp">A tool check</option>
        </select></label>

        {#if kind === 'cron'}
          <CronEditor bind:schedule={cron} disabled={busy} />
        {:else if kind === 'webhook'}
          <label>Sender<select bind:value={preset} disabled={busy || !webhooks}>
            <option value="generic">Any sender</option>
            <option value="github">GitHub</option>
            <option value="forgejo">Forgejo</option>
          </select></label>
          {#if webhooks && tauri && agent}
            {#key created}<WebhookPanel {tauri} runtime={agent.runtimeId} agentId={agent.id} routineId={created?.routine?.id ?? (original?.trigger?.kind === 'webhook' ? original.id : null)} {created} disabled={busy} />{/key}
          {:else}
            {#if trigger.hook_id}<p class="record">Hook {trigger.hook_id}</p>{/if}
            <p class="note">This machine does not issue webhook addresses yet. Update its runtime to add one.</p>
          {/if}
        {:else}
          <div class="line">
            <label class="grow">Server<input class="mono" list={`${id}-servers`} bind:value={server} required disabled={busy} spellcheck="false" autocomplete="off" /></label>
            <label class="grow">Tool<input class="mono" list={`${id}-tools`} bind:value={tool} required disabled={busy} spellcheck="false" autocomplete="off" /></label>
          </div>
          <datalist id={`${id}-servers`}>{#each catalog?.mcp ?? [] as entry (entry.server)}<option value={entry.server}></option>{/each}</datalist>
          <datalist id={`${id}-tools`}>{#each offered.tools as name (name)}<option value={name}></option>{/each}</datalist>
          {#if offered.note}<p class="note">{offered.note}</p>{:else if catalog?.loaded && !catalog.error && !(catalog.mcp ?? []).length}<p class="note">This machine has no MCP servers. Add one in Tools.</p>{/if}
          <label>Arguments<textarea class="mono" rows="3" bind:value={args} disabled={busy} spellcheck="false"></textarea></label>
          <CronEditor bind:schedule={every} label="Check" disabled={busy} />
          <div class="condition"><span class="legend">Start the agent when</span><ConditionBuilder bind:node={condition} disabled={busy} /></div>
        {/if}

        <label>Prompt<textarea rows="4" bind:value={prompt} disabled={busy} placeholder="What the agent does when this starts it"></textarea></label>
        {#if error}<p role="alert">{error}</p>{/if}
      </div>
      <div class="dialog-actions">
        {#if ondelete && original && !created}
          {#if confirmDelete}<Button disabled={busy} onclick={remove}>Confirm delete</Button>{:else}<Button disabled={busy} onclick={() => { confirmDelete = true }}>Delete</Button>{/if}
        {/if}
        <span class="spacer"></span>
        {#if created}
          <Button variant="primary" onclick={onclose}>Done</Button>
        {:else}
          <Button disabled={busy} onclick={onclose}>Cancel</Button>
          <Button type="submit" variant="primary" disabled={busy || !name.trim() || (kind === 'webhook' && !webhooks)}>Save</Button>
        {/if}
      </div>
    </form>
  </div>
</div>

<style>
  .dialog-backdrop { position: fixed; inset: 0; z-index: 10; display: grid; place-items: center; padding: 24px; background: var(--overlay-backdrop); }
  .dialog-panel { display: flex; flex-direction: column; width: min(560px, 100%); max-height: calc(100vh - 48px); font-family: var(--font-human); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 16px 20px 8px; }
  h2 { margin: 0; font-size: var(--text-15); }
  form { display: flex; flex-direction: column; min-height: 0; }
  .body { display: grid; gap: 12px; min-height: 0; overflow-y: auto; padding: 8px 20px 12px; }
  .line { display: flex; flex-wrap: wrap; align-items: end; gap: 8px; }
  .grow { flex: 1 1 160px; }
  label, .condition { display: grid; gap: 4px; min-width: 0; color: var(--muted); font-size: var(--text-12); }
  .legend { color: var(--muted); font-size: var(--text-12); }
  .switch { display: flex; align-items: center; gap: 8px; min-height: 30px; color: var(--ink); }
  input, select, textarea { box-sizing: border-box; width: 100%; min-width: 0; min-height: 30px; padding: 5px 6px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; font-size: var(--text-13); }
  textarea { resize: vertical; line-height: 1.5; }
  .mono, .record { font-family: var(--font-mono); font-size: var(--text-12); }
  .record { margin: 0; color: var(--muted); overflow-wrap: anywhere; }
  .note, [role="alert"] { margin: 0; color: var(--muted); font-size: var(--text-12); line-height: 1.5; }
  [role="alert"] { color: var(--oxide); }
  .dialog-actions { display: flex; align-items: center; gap: 8px; padding: 12px 20px 16px; border-top: 1px solid var(--border); }
  .spacer { flex: 1; }
</style>
