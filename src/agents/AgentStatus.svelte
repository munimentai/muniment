<script>
  // One small mark at the end of an agent's sidebar row for where it lives:
  // a muted dot for a machine that answers, ochre when that machine does not
  // answer, and a ring for a synced copy. Hover on the row and screen readers
  // get the machine, both states and what the agent does now in words.
  import { activityLabel } from './activity.js'
  let { agent, machine = null, activity = undefined } = $props()
  const offline = $derived(agent?.online === false)
  const label = $derived([machine?.name ? `On ${machine.name}` : '', offline ? 'offline' : '', agent?.replica ? 'synced copy' : '', activity && activity.state !== 'idle' ? activityLabel(activity) : ''].filter(Boolean).join(' · '))
</script>

<!-- An `i`, not a `span`: the sidebar row styles each span as its stretching label. -->
<i class="agent-status" data-offline={offline ? 'true' : undefined} data-replica={agent?.replica ? 'true' : undefined} data-tooltip={label}><em class="visually-hidden">{label}</em></i>

<style>
  /* The hover label hangs under the whole row, so the narrow sidebar never clips it. */
  .agent-status { display: block; flex: none; box-sizing: border-box; width: 6px; height: 6px; margin-left: auto; border: 1px solid var(--muted); border-radius: var(--radius-chip); background: var(--muted); }
  .agent-status[data-replica] { background: transparent; }
  .agent-status[data-offline] { border-color: var(--ochre); background: var(--ochre); }
  .agent-status[data-offline][data-replica] { background: transparent; }
  .agent-status::after { content: attr(data-tooltip); position: absolute; z-index: 6; left: 4px; right: 4px; top: calc(100% + 4px); white-space: normal; overflow-wrap: anywhere; padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); line-height: 1.4; pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  :global(.side-action:hover) > .agent-status::after { opacity: 1; transition-delay: 350ms; }
</style>
