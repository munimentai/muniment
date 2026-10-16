<script>
  // One message posted to a thread on a swarm member: a user message that
  // waits for the agent's primary machine, an agent's reply in a group, or a
  // note from Muniment. A meeting round starts with its label.
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import { postAuthor } from './transcript.js'
  import { messageLocalTime } from '../lib/chat-state.js'

  let { post, agents = [], section = null } = $props()
  const author = $derived(postAuthor(post, agents))
</script>

{#if section}<h3 class="round">{section}</h3>{/if}
{#if author.kind === 'system'}
  <p class="system-note" role="note">{post.text}</p>
{:else if author.kind === 'user'}
  <div class="user-post">
    <p class="bubble">{post.text}</p>
    <span class="meta">{post.consumed ? '' : 'Waiting for the agent\'s machine · '}{messageLocalTime(post.posted_at)}</span>
  </div>
{:else}
  <article class="agent-post" aria-label={`Reply from ${author.name}`}>
    <header><AgentAvatar agent={author.agent} size={20} /><strong>{author.name}</strong><time class="meta" datetime={post.posted_at}>{messageLocalTime(post.posted_at)}</time></header>
    <p class="body">{post.text}</p>
  </article>
{/if}

<style>
  .round { margin: 8px 0 0; padding-top: 8px; border-top: 1px solid var(--border); color: var(--muted); font: var(--weight-regular, 400) var(--text-12) var(--font-mono); }
  .system-note { margin: 0; color: var(--muted); font-size: var(--text-13); overflow-wrap: anywhere; }
  .user-post { display: grid; justify-items: end; gap: 4px; }
  .bubble { max-width: min(620px, 85%); margin: 0; padding: 10px 14px; border-radius: var(--radius-panel); background: var(--faint); white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--text-15); line-height: 1.55; }
  .agent-post { display: grid; gap: 6px; }
  header { display: flex; align-items: center; gap: 8px; font-size: var(--text-13); }
  .meta { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .body { margin: 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--text-15); line-height: 1.55; }
</style>
