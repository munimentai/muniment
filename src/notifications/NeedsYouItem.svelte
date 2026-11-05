<script>
  // One item that needs the user. An approval is its approval card with the
  // answers core accepts, a question takes a reply posted to the run's
  // thread, a run a usage limit stopped names the reset and runs again once
  // it passes, a failed run names its error with Open thread, and Open
  // Models when the fix is there, and a machine that does not answer says so.
  import ApprovalCard from '../agents/ApprovalCard.svelte'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import Button from '../lib/ui/Button.svelte'
  import MachineChip from '../runtimes/MachineChip.svelte'
  import { agentRequest } from '../agents/agent-runtime.js'
  import { memberMachine } from '../agents/swarm.js'
  import { resetsIn } from '../usage/usage.js'
  import { runFailureRaw, runFailureText } from '../lib/run-failure.js'
  import { clockTime, needsMark, pointsToModels, runWithMachine } from './notifications.js'
  import { DELETED_THREAD_NOTE, threadDeleted } from '../lib/deleted-threads.svelte.js'

  let { tauri, item, agents = [], swarm = null, runtimeList = null, showMachine = false, now = Date.now(), onopen = () => {}, onanswered = () => {}, onapprovalopen = null, onmodels = null } = $props()
  const mark = $derived(needsMark(item.kind))
  const agent = $derived(agents.find((entry) => entry.id === item.agent_id) ?? (item.agent_id ? { id: item.agent_id, name: item.agent_name } : null))
  // A failed run reads as its thread reply reads, with a stored raw answer behind Show log.
  const failure = $derived(item.kind === 'failed' ? runFailureText(item.detail) : '')
  const failureLog = $derived(item.kind === 'failed' ? runFailureRaw(item.detail) : [])
  // A run whose thread was deleted keeps its item, says so, and turns Open thread off.
  const gone = $derived(threadDeleted(item.run?.thread_id))
  const resetPassed = $derived(Number.isFinite(item.resets_at_ms) && item.resets_at_ms <= now)
  let reply = $state('')
  let busy = $state(false)
  let note = $state('')

  async function send() {
    const text = reply.trim()
    if (!text || busy) return
    busy = true
    note = ''
    try {
      await agentRequest(tauri, item.machine.id, 'thread.post', { thread_id: item.run.thread_id, text })
      reply = ''
      note = 'Reply sent.'
      onanswered(item)
    } catch (error) {
      note = String(error?.message ?? error)
    } finally {
      busy = false
    }
  }

  async function runAgain() {
    busy = true
    note = ''
    try {
      await agentRequest(tauri, item.machine.id, 'agent.run_now', { id: item.agent_id })
      note = 'Queued to run again.'
      onanswered(item)
    } catch (error) {
      note = String(error?.message ?? error)
    } finally {
      busy = false
    }
  }
</script>

{#if item.kind === 'approval'}
  <ApprovalCard {tauri} approval={item.approval} {agent} machine={memberMachine(swarm, item.approval?.runtime_id, runtimeList)} onopen={onapprovalopen} onanswered={() => onanswered(item)} />
{:else}
  <article class="needs" data-kind={item.kind} aria-label={`${mark.title}${item.agent_name ? `: ${item.agent_name}` : `: ${item.machine.name}`}`}>
    <span class="mark"><LucideIcon name={mark.icon} size={16} variant="action" /></span>
    <div class="body">
      <p class="head">
        {#if agent}<AgentAvatar {agent} size={18} tooltip={false} />{/if}
        <strong>{item.kind === 'offline' ? item.machine.name : item.agent_name || 'Agent'}</strong>
        <span class="kind">{mark.title}</span>
        {#if showMachine && item.kind !== 'offline'}<MachineChip machine={item.machine} {runtimeList} />{/if}
        {#if item.at}<time datetime={item.at}>{clockTime(item.at)}</time>{/if}
      </p>
      {#if item.kind === 'offline'}
        <p class="detail">Its runs and approvals show when it answers again.</p>
      {:else if item.kind === 'limit'}
        <p class="detail">{Number.isFinite(item.resets_at_ms) ? (resetPassed ? 'The allowance reset.' : `${resetsIn(item.resets_at_ms, now)}.`) : 'Wait for the reset or add an account.'}</p>
        <div class="actions">
          <Button icon="play" disabled={busy || (Number.isFinite(item.resets_at_ms) && !resetPassed)} onclick={runAgain}>{resetPassed || !Number.isFinite(item.resets_at_ms) ? 'Run again' : 'Run again at reset'}</Button>
          <Button disabled={gone} onclick={() => onopen(runWithMachine(item.run, item.machine))}>Open thread</Button>
        </div>
      {:else if item.kind === 'failed'}
        <p class="detail">{failure || 'The run failed.'}</p>
        {#if failureLog.length}<details class="log"><summary>Show log</summary>{#each failureLog as entry, index (index)}<pre>{entry}</pre>{/each}</details>{/if}
        <div class="actions">
          {#if onmodels && pointsToModels(failure)}<Button icon="settings" onclick={() => onmodels()}>Open Models</Button>{/if}
          <Button disabled={!item.run?.thread_id || gone} onclick={() => onopen(runWithMachine(item.run, item.machine))}>Open thread</Button>
        </div>
      {:else}
        {#if item.detail}<p class="detail">{item.detail}</p>{/if}
        <form class="reply" onsubmit={(event) => { event.preventDefault(); void send() }}>
          <input aria-label={`Reply to ${item.agent_name || 'the agent'}`} placeholder="Reply" bind:value={reply} disabled={busy} />
          <Button type="submit" disabled={busy || !reply.trim()}>Send</Button>
          <Button disabled={gone} onclick={() => onopen(runWithMachine(item.run, item.machine))}>Open thread</Button>
        </form>
      {/if}
      {#if gone}<p class="note">{DELETED_THREAD_NOTE}</p>{/if}
      {#if note}<p class="note" role="status">{note}</p>{/if}
    </div>
  </article>
{/if}

<style>
  .needs { display: flex; gap: 10px; padding: 12px 0; }
  .mark { flex: none; display: inline-flex; padding-top: 1px; color: var(--warning); }
  [data-kind="offline"] .mark, [data-kind="failed"] .mark { color: var(--danger); }
  .body { display: grid; flex: 1; gap: 6px; min-width: 0; }
  .head { display: flex; flex-wrap: wrap; align-items: center; gap: 6px 8px; margin: 0; color: var(--ink); font-size: var(--text-13); }
  .kind { color: var(--warning); font-size: var(--text-12); }
  [data-kind="offline"] .kind, [data-kind="failed"] .kind { color: var(--danger); }
  time { margin-left: auto; color: var(--muted); font-size: var(--text-12); }
  .detail, .note { margin: 0; color: var(--muted); font-size: var(--text-13); overflow-wrap: anywhere; }
  .log { color: var(--muted); font-size: var(--text-12); }
  .log summary { cursor: pointer; width: max-content; }
  .log pre { margin: 4px 0 0; padding: 8px; overflow-x: auto; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--faint); font: var(--text-12) var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  .actions, .reply { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .reply input { flex: 1; min-width: 160px; min-height: 32px; box-sizing: border-box; padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
</style>
