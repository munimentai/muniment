<script>
  import { composerParts, referencePrefix } from './composer-references.js'
  import { EVERYONE } from '../agents/mentions.js'
  import AgentAvatar from '../lib/AgentAvatar.svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import FileIcon from '../files/FileIcon.svelte'
  // The textarea above draws the caret and takes the input. This layer draws the same text in the same
  // boxes, so every glyph and line break matches. An icon covers the characters it stands for, a link's
  // scheme or a reference's @, which stay in the text as transparent glyphs and show again while the
  // selection touches that part. `files` are the selected files, `agents` the group members the draft
  // can name, and `selection` the textarea's selection, or null while it has no focus.
  let { text = '', files = [], agents = [], commands = [], selection = null, scrollTop = 0, scrollLeft = 0, width } = $props()
  let overlay = $state()
  const named = $derived(agents.length ? [EVERYONE, ...agents] : [])
  const parts = $derived(composerParts(text, files.map((file) => file.referenceName), commands, named.map((agent) => agent.name)))
  function mark(part) {
    const prefix = referencePrefix(part)
    if (part.type === 'link') return { prefix }
    if (part.type === 'file') { const file = files.find((item) => item.referenceName && `@${item.referenceName}` === part.text); return file && { prefix, file } }
    if (part.type === 'agent') { const agent = named.find((item) => `@${item.name}` === part.text); return agent && { prefix, agent } }
    return null
  }
  const editing = (part) => !!selection && selection.start <= part.start + part.text.length && selection.end >= part.start
  // The link under a viewport point, for a modifier click on the textarea above.
  export function linkAt(x, y) {
    for (const node of overlay?.querySelectorAll('[data-url]') ?? []) for (const rect of node.getClientRects()) if (x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom) return node.dataset.url
    return null
  }
</script>
<div class="reference-overlay" aria-hidden="true" bind:this={overlay}><div style:width={width ? `${width}px` : undefined} style:transform={`translate(${-scrollLeft}px, ${-scrollTop}px)`}>{#each parts as part, i (i)}{@const icon = part.type === 'text' ? null : mark(part)}{#if part.type === 'text'}<span>{part.text}</span>{:else if icon}<span class="reference" class:link={part.type === 'link'} class:agent={part.type === 'agent'} class:editing={editing(part)} data-url={part.type === 'link' ? part.text : undefined}><span class="prefix">{icon.prefix}<span class="mark">{#if icon.file}<FileIcon name={icon.file.referenceName} directory={icon.file.isDirectory} />{:else if icon.agent?.everyone}<LucideIcon name="users" variant="action" />{:else if icon.agent}<AgentAvatar agent={icon.agent} size={14} tooltip={false} />{:else}<LucideIcon name="globe" variant="action" />{/if}</span></span>{part.text.slice(icon.prefix.length)}</span>{:else}<span class="reference">{part.text}</span>{/if}{/each}{'\n'}</div></div>
<style>
  /* Kerning and ligatures are off here and in the textarea: a span edge, such as after a hidden scheme, would drop a kerning pair and move every later glyph. */
  .reference-overlay { position: absolute; inset: 0; pointer-events: none; overflow: hidden; color: var(--ink); white-space: pre-wrap; overflow-wrap: break-word; font: inherit; line-height: inherit; font-kerning: none; font-variant-ligatures: none; }
  .reference { color: var(--reference); }
  /* The icon box is the hidden characters' own box, so it never moves a glyph or a line break. */
  .prefix { position: relative; }
  .reference:not(.editing) .prefix { color: transparent; }
  .mark { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; line-height: 0; color: var(--reference); }
  .editing .mark { visibility: hidden; }
  .link .mark { justify-content: flex-end; padding-right: .2em; }
  .mark :global(.lucide) { width: 1em; height: 1em; }
  .agent .mark :global(.lucide) { width: .9em; height: .9em; }
  @media (forced-colors: active) { .reference, .mark { color: LinkText; } }
</style>
