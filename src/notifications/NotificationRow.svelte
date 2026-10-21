<script>
  // One run on the page: a status icon in the state's color, the agent, what
  // started it with its icon, its machine with its stoplight, one line of its
  // result and its time. The row opens the run's thread.
  import LucideIcon from '../lib/LucideIcon.svelte'
  import MachineChip from '../runtimes/MachineChip.svelte'
  import { machineState, runtimeFor } from '../runtimes/runtimes.js'
  import { clockTime, resultLine, runWithMachine, statusMark, triggerIcon, triggerLabel } from './notifications.js'
  let { entry, showMachine = false, runtimeList = null, onopen = () => {} } = $props()
  const mark = $derived(statusMark(entry.status))
  const runtime = $derived(showMachine ? runtimeFor(runtimeList, entry.machine) : null)
  const where = $derived(showMachine ? `, on ${entry.machine.name}${runtime ? `, ${machineState(runtime)}` : ''}` : '')
  const at = $derived(entry.run.updated_at)
</script>

<li class="notification-row" data-status={entry.status} data-unread={entry.unread ? 'true' : undefined}>
  <button type="button" disabled={!entry.run.thread_id} onclick={() => onopen(runWithMachine(entry.run, entry.machine))} aria-label={`${entry.agent_name || 'Agent'}, ${mark.label}${entry.unread ? ', unread' : ''}${where}: ${resultLine(entry)}`}>
    <span class="status"><LucideIcon name={mark.icon} size={16} variant="action" /></span>
    <span class="main">
      <span class="line">
        <strong>{entry.agent_name || 'Agent'}</strong>
        <span class="chip"><LucideIcon name={triggerIcon(entry.run.trigger)} variant="action" size={12} />{triggerLabel(entry.run.trigger)}</span>
        {#if showMachine}<MachineChip machine={entry.machine} {runtimeList} />{/if}
      </span>
      <span class="result">{resultLine(entry)}</span>
    </span>
    <time datetime={at}>{clockTime(at)}</time>
  </button>
</li>

<style>
  .notification-row { list-style: none; }
  .notification-row + :global(.notification-row) { border-top: 1px solid var(--border); }
  button { display: flex; align-items: flex-start; gap: 10px; width: 100%; padding: 10px 4px; border: 0; border-radius: var(--radius-control); background: transparent; color: inherit; font: inherit; text-align: left; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { cursor: default; }
  .status { flex: none; display: inline-flex; padding-top: 1px; color: var(--success); }
  [data-status="running"] .status { color: var(--active); }
  [data-status="needs_you"] .status { color: var(--warning); }
  [data-status="failed"] .status { color: var(--danger); }
  [data-status="cancelled"] .status { color: var(--muted); }
  .main { display: grid; flex: 1; gap: 2px; min-width: 0; }
  .line { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; min-width: 0; }
  strong { color: var(--ink); font: 600 var(--text-13) var(--font-human); }
  [data-unread="true"] strong::after { content: ''; display: inline-block; width: 6px; height: 6px; margin-left: 6px; border-radius: var(--radius-chip); background: var(--ink); vertical-align: middle; }
  .chip { display: inline-flex; align-items: center; gap: 4px; padding: 0 6px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font-size: var(--text-12); line-height: 18px; }
  .result { overflow: hidden; color: var(--muted); font-size: var(--text-13); text-overflow: ellipsis; white-space: nowrap; }
  time { flex: none; color: var(--muted); font-size: var(--text-12); }
</style>
