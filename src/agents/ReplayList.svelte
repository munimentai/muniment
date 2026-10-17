<script>
  // Failed runs of one agent that keep their input: when each failed, what
  // started it, its error, the saved input folded, and Replay, which runs it
  // again against the agent as it is now. A queued replay links its run once
  // the run starts its conversation.
  import Button from '../lib/ui/Button.svelte'
  import { runStatusLabel, whenLabel } from '../lib/agent-labels.js'
  import { loadReplays, replayNotes, replayRun, replaySource, replayedRun } from './replays.js'

  let { tauri, agent, runs = [], disabled = false, onrun = () => {}, onopen = null, onactivity = null } = $props()
  let replays = $state([])
  let loading = $state(true)
  let error = $state('')
  let busy = $state('')
  // The run each replay queued from this profile, by replay id.
  let queued = $state({})

  async function load() {
    loading = true
    const answer = await loadReplays(tauri, agent)
    replays = answer.replays
    error = answer.error
    loading = false
  }
  $effect(() => { agent.id; void load() })

  async function replay(entry) {
    busy = entry.replay_id
    error = ''
    try {
      const answer = await replayRun(tauri, agent, entry)
      replays = replays.map((item) => item.replay_id === entry.replay_id ? { ...item, ...answer.replay } : item)
      if (answer.run) { queued = { ...queued, [entry.replay_id]: answer.run }; onrun(answer.run) }
    } catch (failure) {
      error = String(failure?.message ?? failure)
    } finally {
      busy = ''
    }
  }
</script>

<section class="field" aria-label="Failed runs">
  <div class="field-heading"><h3>Failed runs</h3>{#if replays.length}<span class="count">{replays.length}</span>{/if}</div>
  {#if loading}<p class="muted">Loading failed runs…</p>
  {:else if !replays.length && !error}
    <div class="empty"><p class="muted">No failed runs to replay.</p>{#if onactivity}<Button onclick={onactivity}>Open activity</Button>{/if}</div>
  {:else}
    <ul>
      {#each replays as entry (entry.replay_id)}
        {@const run = replayedRun(runs, queued[entry.replay_id])}
        <li class="replay">
          <div class="line"><strong>{replaySource(entry, agent)}</strong><time class="record" datetime={entry.failed_at}>{whenLabel(entry.failed_at)}</time></div>
          {#if entry.error}<p class="error-line">{entry.error}</p>{/if}
          {#each replayNotes(entry) as note (note)}<p class="muted">{note}</p>{/each}
          {#if entry.prompt_preview}<details><summary>Input</summary><pre>{entry.prompt_preview}</pre>{#if entry.prompt_bytes > entry.prompt_preview.length}<p class="muted">The preview shows the start of {entry.prompt_bytes} bytes.</p>{/if}</details>{/if}
          <div class="actions">
            <Button disabled={disabled || !!busy} onclick={() => replay(entry)}>{busy === entry.replay_id ? 'Replaying…' : 'Replay'}</Button>
            {#if run}
              <span class="record">Replay {runStatusLabel(run.status).toLowerCase()}</span>
              {#if run.thread_id && onopen}<Button variant="outline" onclick={() => onopen(run)}>Open run</Button>{:else}<span class="muted">It links here once it starts.</span>{/if}
            {/if}
          </div>
        </li>
      {/each}
    </ul>
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>

<style>
  .field { display: grid; gap: 6px; padding: 12px 0; border-bottom: 1px solid var(--border); }
  .field-heading { display: flex; align-items: center; gap: 8px; min-height: 24px; }
  h3 { margin: 0; color: var(--muted); font-size: var(--text-12); font-weight: 500; }
  .count { color: var(--muted); font: var(--text-12) var(--font-mono); }
  ul { display: grid; gap: 0; margin: 0; padding: 0; list-style: none; }
  .replay { display: grid; gap: 4px; padding: 8px 0; }
  .replay + .replay { border-top: 1px solid var(--border); }
  .line { display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: 8px; font-size: var(--text-13); }
  p { margin: 0; font-size: var(--text-12); line-height: 1.5; overflow-wrap: anywhere; }
  .error-line { color: var(--oxide); }
  .muted { color: var(--muted); font-size: var(--text-12); }
  .record { color: var(--muted); font: var(--text-12) var(--font-mono); }
  details { font-size: var(--text-12); }
  summary { cursor: pointer; color: var(--muted); }
  pre { margin: 4px 0 0; padding: 8px; border-radius: var(--radius-control); background: var(--faint); font: var(--text-12) var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  .actions, .empty { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; margin-top: 2px; }
  [role="alert"] { margin: 0; color: var(--oxide); font-size: var(--text-12); }
</style>
