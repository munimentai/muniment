<script>
  // One part of a Settings page that holds several, such as Context on Data.
  // The part's own head shows `title` one level under the page title, and its
  // groups sit one level under that. `note` says where the part keeps its
  // settings when that differs from the machine Settings names, under its head. A link or a search
  // result that names the part scrolls it into view. The part's own content
  // names its region, so the wrapper adds no landmark of its own.
  import { setContext } from 'svelte'
  let { id, title, note = '', focus = false, children } = $props()
  // The part's head reads its title and note from this context, and a note
  // that arrives later shows under the head.
  setContext('page-part', { get title() { return title }, get note() { return note } })
  let node = $state()
  // Parts above this one load their sections after it scrolls and push it
  // down, so it scrolls again as the page grows until the user scrolls,
  // clicks or types, or for two seconds.
  $effect(() => {
    if (!focus || !node) return
    const scroll = () => node.scrollIntoView?.({ block: 'start' })
    scroll()
    const page = node.parentElement
    if (!page || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(scroll)
    observer.observe(page)
    const stop = () => observer.disconnect()
    const events = ['wheel', 'touchstart', 'pointerdown', 'keydown']
    for (const name of events) window.addEventListener(name, stop, { capture: true, once: true })
    const timer = setTimeout(stop, 2000)
    return () => { stop(); clearTimeout(timer); for (const name of events) window.removeEventListener(name, stop, { capture: true }) }
  })
</script>

<div class="settings-part" data-settings-part={id} bind:this={node}>
  {@render children?.()}
</div>

<style>
  .settings-part { display: grid; gap: var(--page-gap); min-width: 0; scroll-margin-top: var(--page-pad); }
  .settings-part + :global(.settings-part) { padding-top: var(--page-gap); border-top: 1px solid var(--border); }
</style>
