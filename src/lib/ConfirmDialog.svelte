<script module>
  // The open confirms, newest last. Escape answers only the topmost.
  const open = []
</script>

<script>
  import { dialogDismiss } from './dialog-dismiss.js'
  import PopupClose from './PopupClose.svelte'
  import { onMount, tick } from 'svelte'
  import { focusLikeKeys } from './focus-source.js'

  // `destructive` marks a confirm that removes something with the danger
  // treatment, as in ui/Button. Cancel stays the focused, safe choice.
  // `opener` is the control that asked. Closing returns focus to it, since a
  // click in WebKit leaves focus on the page, not on the button it pressed.
  let { title, children, onDecision, cancelLabel = 'Deny', confirmLabel = 'Allow', destructive = false, opener = null } = $props()
  let dialogPanel
  let denyButton
  let pending = $state(false)

  onMount(() => {
    const previousFocus = opener ?? document.activeElement
    void tick().then(() => focusLikeKeys(denyButton))
    const self = {}
    open.push(self)

    // Escape answers Cancel from anywhere in the window. The listener captures
    // on the window, so a page under the dialog, such as Settings, never takes
    // the key first and closes itself instead.
    const onKeydown = (event) => {
      if (open.at(-1) !== self) return
      if (event.key === 'Escape') {
        event.preventDefault()
        event.stopImmediatePropagation()
        if (!pending) void decide(false)
        return
      }
      if (event.key !== 'Tab') return

      const controls = [...dialogPanel.querySelectorAll('button:not(:disabled)')]
      if (controls.length === 0) {
        event.preventDefault()
        return
      }

      const first = controls[0]
      const last = controls.at(-1)
      if (event.shiftKey && (document.activeElement === first || !dialogPanel.contains(document.activeElement))) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && (document.activeElement === last || !dialogPanel.contains(document.activeElement))) {
        event.preventDefault()
        first.focus()
      }
    }
    window.addEventListener('keydown', onKeydown, true)

    return () => {
      window.removeEventListener('keydown', onKeydown, true)
      open.splice(open.indexOf(self), 1)
      focusLikeKeys(previousFocus)
    }
  })

  // The dialog sits on the page body, above every panel. Inside a page it
  // shares that page's stacking and scroll, so the app's sidebar and title
  // bar draw over it and take a real pointer's click, which no keyboard or
  // accessibility press meets.
  function portal(node) {
    document.body.appendChild(node)
    return { destroy() { node.remove() } }
  }

  async function decide(approve) {
    if (pending) return
    pending = true
    try { await onDecision(approve) } finally { pending = false }
  }
</script>

<div class="dialog-backdrop" use:portal use:dialogDismiss={{onclose: () => decide(false), disabled: pending}}>
  <div data-panel="confirm" data-panel-variant="overlay" bind:this={dialogPanel} class="dialog-panel" role="dialog" aria-modal="true" aria-labelledby="confirm-dialog-title">
    <header><h2 id="confirm-dialog-title">{title}</h2><PopupClose label="Close confirmation" disabled={pending} onclick={() => decide(false)} /></header>
    <div class="dialog-copy">{@render children()}</div>
    <div class="dialog-actions">
      <!-- data-ui-button keeps a host's control styles, such as settings-controls, off the decision buttons. -->
      <button type="button" data-ui-button bind:this={denyButton} disabled={pending} onclick={() => decide(false)}>{cancelLabel}</button>
      <button type="button" data-ui-button class="allow" class:danger={destructive} disabled={pending} onclick={() => decide(true)}>{confirmLabel}</button>
    </div>
  </div>
</div>

<style>
  .dialog-backdrop { position: fixed; inset: 0; z-index: 10; display: grid; place-items: center; padding: 24px; background: var(--overlay-backdrop); }
  .dialog-panel { width: min(440px, 100%); padding: 24px;      font-family: var(--font-human); }
  header { display: flex; align-items: start; justify-content: space-between; gap: 16px; }
  h2 { margin: 0; font-size: var(--text-22); line-height: var(--leading-heading); letter-spacing: var(--tracking-heading); }
  .dialog-copy { margin-top: 12px; color: var(--muted); font-size: var(--text-15); line-height: var(--leading-body); overflow-wrap: anywhere; }
  .dialog-copy :global(p) { margin: 0; }
  .dialog-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 24px; }
  button { min-height: 36px; padding: 7px 18px; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--weight-semibold) var(--text-13) var(--font-human); }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { opacity: .55; }
  .allow { border-color: var(--ink); background: var(--ink); color: var(--paper); }
  .allow:hover:not(:disabled) { opacity: .9; background: var(--ink); }
  .allow.danger { border-color: var(--danger-edge); background: var(--danger-soft); color: var(--danger); }
  .allow.danger:hover:not(:disabled) { opacity: 1; border-color: var(--danger); background: color-mix(in srgb, var(--danger) 20%, var(--surface)); }
</style>
