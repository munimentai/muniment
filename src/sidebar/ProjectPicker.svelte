<script>
  // Move to project: every project on this computer and No project, the
  // thread's own choice checked. A pick moves the thread, and its next run
  // works in that project's folder with its instructions and tools. No
  // project makes the thread a project of its own.
  import { onMount, tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  import PopupClose from '../lib/PopupClose.svelte'
  import { dialogDismiss } from '../lib/dialog-dismiss.js'
  import { followPointer } from '../lib/menu-pointer.js'

  // `projects` are [{ id, name }], with No project as id null.
  let { title, projects = [], current, busy = false, error = '', onmove, onclose } = $props()
  let list = $state()

  onMount(() => {
    const previous = document.activeElement
    void tick().then(() => (list?.querySelector('button:not(:disabled)'))?.focus())
    return () => { if (previous?.isConnected) previous.focus?.() }
  })

  function keydown(event) {
    if (event.key === 'Escape' && !busy) { event.preventDefault(); event.stopPropagation(); onclose(); return }
    if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
    event.preventDefault()
    const buttons = [...list.querySelectorAll('button:not(:disabled)')]
    const index = buttons.indexOf(document.activeElement)
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length
    buttons[next]?.focus()
  }
</script>

<div class="picker-backdrop" use:dialogDismiss={{ onclose, disabled: busy }}>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div data-panel="move-project" data-panel-variant="overlay" class="project-picker" role="dialog" aria-modal="true" aria-labelledby="move-project-title" tabindex="-1" onkeydown={keydown}>
    <header><h2 id="move-project-title">Move to project</h2><PopupClose label="Close move to project" disabled={busy} onclick={onclose} /></header>
    <p class="support">{title}</p>
    <div class="choices" role="group" aria-label="Projects" bind:this={list} use:followPointer={'.choice'}>
      {#each projects as project (project.id)}
        <button type="button" class="choice" aria-current={project.id === current ? 'true' : undefined} disabled={busy || project.id === current} onclick={() => onmove(project.id)}>
          <LucideIcon name={project.id === null ? 'message-square' : 'folder'} size={16} />
          <span>{project.name}</span>
          {#if project.id === current}<LucideIcon name="check" size={14} />{/if}
        </button>
      {/each}
    </div>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
</div>

<style>
  .picker-backdrop { position: fixed; inset: 0; z-index: 30; display: flex; justify-content: center; align-items: flex-start; padding: 14vh 16px 16px; background: var(--overlay-backdrop); }
  .project-picker { width: min(360px, 100%); max-height: min(480px, 72vh); display: flex; flex-direction: column; padding: 16px; outline: 0; font-family: var(--font-human); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  h2 { margin: 0; font-size: var(--text-17); line-height: var(--leading-heading); }
  .support { margin: 4px 0 10px; overflow: hidden; color: var(--muted); font-size: var(--text-13); white-space: nowrap; text-overflow: ellipsis; }
  .choices { display: grid; gap: 1px; min-height: 0; overflow-y: auto; }
  .choice { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 30px; padding: 5px 8px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: pointer; }
  .choice :global(svg) { flex: none; color: var(--muted); }
  .choice span { flex: 1; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .choice:hover:not(:disabled), .choice:focus-visible { background: var(--faint); }
  .choice[aria-current="true"] { background: var(--selected); color: var(--on-selected); cursor: default; }
  .choice:disabled:not([aria-current="true"]) { opacity: .55; cursor: default; }
  .error { margin: 8px 0 0; color: var(--danger); font-size: var(--text-13); }
</style>
