<script>
  import { avatarSvg, avatarFor } from './agent-avatar.js'
  import { avatarDisplay } from './avatar-display.svelte.js'
  import { activityLabel, activityState } from '../agents/activity.js'
  // The one agent avatar, on every surface. It is decorative: the agent's name always
  // sits beside it. It draws the head alone in the avatar style picked in Appearance and
  // the tone of the theme in use, and redraws when either changes.
  // `activity`, when given, outlines the head with what the agent does now and names it
  // on hover: a breathing signal outline while it runs, warning while it waits for an
  // answer and danger after a failed run. Idle shows no outline.
  // `tooltip` false leaves the hover label to the surface, such as a sidebar row
  // whose own mark names the activity where the avatar's label would be clipped.
  let { agent, size = 24, activity = undefined, tooltip = true } = $props()
  const display = avatarDisplay()
  const state = $derived(activity === undefined ? null : activityState(activity))
</script>
<!-- The generators draw fixed local geometry and never insert user text. -->
<span class="agent-avatar" aria-hidden="true" data-activity={state} data-tooltip={state && tooltip ? activityLabel(activity) : undefined} style:width="{size}px" style:height="{size}px">{@html avatarSvg(avatarFor(agent), display)}</span>
<style>
  /* A fixed square box that centers in its slot. The head fills it with no baseline gap
     and no tile, so it sits on the surface behind it.
     The doubled class outranks a parent's `> span` rule, such as the catalog card's text clamp. */
  .agent-avatar.agent-avatar { display: block; flex: none; align-self: center; box-sizing: border-box; line-height: 0; }
  .agent-avatar.agent-avatar :global(svg) { display: block; width: 100%; height: 100%; overflow: visible; }
  /* The outline follows the head's own shape, so the box and its drawing keep their size. */
  .agent-avatar.agent-avatar[data-activity] :global(svg) { filter: drop-shadow(1px 0 0 var(--activity)) drop-shadow(-1px 0 0 var(--activity)) drop-shadow(0 1px 0 var(--activity)) drop-shadow(0 -1px 0 var(--activity)); }
  .agent-avatar.agent-avatar[data-activity="idle"] :global(svg) { filter: none; }
  .agent-avatar.agent-avatar[data-activity="running"] { --activity: var(--signal); animation: avatar-running 2.4s ease-in-out infinite; }
  .agent-avatar.agent-avatar[data-activity="waiting"] { --activity: var(--warning); }
  .agent-avatar.agent-avatar[data-activity="error"] { --activity: var(--danger); }
  /* The hover label names what the agent does now. */
  .agent-avatar.agent-avatar[data-tooltip] { position: relative; }
  .agent-avatar.agent-avatar[data-tooltip]::after { content: attr(data-tooltip); position: absolute; z-index: 6; left: 0; bottom: calc(100% + 6px); width: max-content; max-width: 280px; padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); line-height: 1.4; white-space: normal; pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  .agent-avatar.agent-avatar[data-tooltip]:hover::after { opacity: 1; transition-delay: 350ms; }
  @property --activity { syntax: '<color>'; inherits: true; initial-value: transparent; }
  @keyframes avatar-running { 50% { --activity: transparent; } }
  :global(:root[data-motion="reduce"]) .agent-avatar.agent-avatar[data-activity="running"] { animation: none; }
</style>
