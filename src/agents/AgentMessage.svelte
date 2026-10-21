<script>
  // A message another agent sent into this thread, or a handoff to it, with
  // its sender, its intent and how many agent hops brought it here. Its text
  // is an agent's reply, so it renders as the sanitized Markdown replies use.
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import AssistantMarkdown from '../lib/AssistantMarkdown.svelte'
  import { hopLabel, intentLabel } from './transcript.js'
  import { formatClock } from '../lib/time-format.svelte.js'

  let { envelope, agent = null, sentAt = '', tauri = null, onopenlink = undefined } = $props()
  const sender = $derived(agent ?? { id: envelope.agentId ?? envelope.from, name: envelope.from })
</script>

{#if envelope.kind === 'handoff'}
  <div class="system-note" role="note">
    <p><strong>{envelope.from}</strong> handed this task over: {envelope.text}</p>
    {#if envelope.context}<details><summary>Context</summary><p class="body">{envelope.context}</p></details>{/if}
    <span class="meta">{hopLabel(envelope.hop)}</span>
  </div>
{:else}
  <article class="agent-message" aria-label={`Message from ${envelope.from}`}>
    <header>
      <AgentAvatar agent={sender} size={20} />
      <strong>{envelope.from}</strong>
      <span class="chip">{intentLabel(envelope.intent)}</span>
      <span class="meta">{hopLabel(envelope.hop)}</span>
      {#if sentAt}<time class="meta" datetime={sentAt}>{formatClock(sentAt)}</time>{/if}
    </header>
    <div class="body"><AssistantMarkdown {tauri} {onopenlink} text={envelope.text} /></div>
  </article>
{/if}

<style>
  .agent-message { display: grid; gap: 6px; max-width: min(620px, 100%); padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); }
  header { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-width: 0; font-size: var(--text-13); }
  .chip { padding: 1px 6px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--ink); font: var(--text-12) var(--font-mono); }
  .meta { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .body { margin: 0; overflow-wrap: anywhere; font-size: var(--text-15); line-height: 1.55; }
  .system-note .body { white-space: pre-wrap; }
  .system-note { display: grid; gap: 4px; justify-items: start; padding: 4px 0; color: var(--muted); font-size: var(--text-13); }
  .system-note p { margin: 0; overflow-wrap: anywhere; }
  .system-note .body { font-size: var(--text-13); }
  details summary { cursor: pointer; font-size: var(--text-12); }
</style>
