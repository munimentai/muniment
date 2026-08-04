<script>
  import { onMount, tick } from 'svelte'

  let { title, children, onDecision } = $props()
  let dialogPanel
  let denyButton
  let pending = $state(false)

  onMount(() => {
    const previousFocus = document.activeElement
    void tick().then(() => denyButton?.focus())

    const onKeydown = (event) => {
      if (event.key === 'Escape' && !pending) {
        event.preventDefault()
        void decide(false)
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
    document.addEventListener('keydown', onKeydown)

    return () => {
      document.removeEventListener('keydown', onKeydown)
      previousFocus?.focus?.()
    }
  })

  async function decide(approve) {
    if (pending) return
    pending = true
    await onDecision(approve)
  }
</script>

<div class="dialog-backdrop">
  <div bind:this={dialogPanel} class="dialog-panel" role="dialog" aria-modal="true" aria-labelledby="confirm-dialog-title">
    <h2 id="confirm-dialog-title">{title}</h2>
    <div class="dialog-copy">{@render children()}</div>
    <div class="dialog-actions">
      <button bind:this={denyButton} disabled={pending} onclick={() => decide(false)}>Deny</button>
      <button class="allow" disabled={pending} onclick={() => decide(true)}>Allow</button>
    </div>
  </div>
</div>

<style>
  .dialog-backdrop { position: fixed; inset: 0; z-index: 10; display: grid; place-items: center; padding: 24px; background: color-mix(in srgb, var(--ink) 28%, transparent); }
  .dialog-panel { width: min(440px, 100%); padding: 24px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); color: var(--ink); box-shadow: var(--shadow-overlay); font-family: var(--font-human); }
  h2 { margin: 0; font-size: var(--text-22); line-height: var(--leading-heading); letter-spacing: var(--tracking-heading); }
  .dialog-copy { margin-top: 12px; color: var(--muted); font-size: var(--text-15); line-height: var(--leading-body); overflow-wrap: anywhere; }
  .dialog-copy :global(p) { margin: 0; }
  .dialog-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 24px; }
  button { min-height: 36px; padding: 7px 18px; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--weight-semibold) var(--text-13) var(--font-human); }
  button:hover:not(:disabled) { background: var(--faint); }
  button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  button:disabled { opacity: .55; }
  .allow { border-color: var(--ink); background: var(--ink); color: var(--paper); }
  .allow:hover:not(:disabled) { opacity: .9; background: var(--ink); }
</style>
