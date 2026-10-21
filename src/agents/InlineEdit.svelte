<script>
  // A text value that edits in place. Hover or focus shows a pencil at its
  // right end, over the end of the text. A click on the text or the pencil
  // opens the field; Enter or leaving the field saves, Escape cancels. A
  // failed save keeps the field open with the refusal under it.
  import { tick } from 'svelte'
  import LucideIcon from '../lib/LucideIcon.svelte'
  let { value = '', label, placeholder = '', maxlength = 120, required = false, disabled = false, strong = false, onsave } = $props()
  let editing = $state(false)
  let draft = $state('')
  let busy = $state(false)
  let error = $state('')
  let input = $state()
  let opener = $state()

  export async function open() {
    if (disabled) return
    draft = value ?? ''
    error = ''
    editing = true
    await tick()
    input?.focus()
    input?.select()
  }
  async function close() {
    editing = false
    error = ''
    await tick()
    opener?.focus()
  }
  async function save() {
    if (busy || !editing) return
    const next = draft.trim()
    if (next === (value ?? '').trim() || (required && !next)) return close()
    busy = true
    error = ''
    try {
      await onsave(next)
      busy = false
      await close()
    } catch (failure) {
      error = String(failure?.message ?? failure)
      busy = false
      await tick()
      input?.focus()
    }
  }
  function keydown(event) {
    if (event.key === 'Enter') { event.preventDefault(); void save() }
    if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); if (!busy) void close() }
  }
  // Leaving the field saves, unless a failed save just showed its reason.
  function blur() {
    if (!error) void save()
  }
</script>

<div class="inline-edit" class:strong>
  {#if editing}
    <input bind:this={input} bind:value={draft} aria-label={label} {maxlength} {placeholder} disabled={busy} onkeydown={keydown} onblur={blur} />
  {:else}
    <button type="button" class="value" class:empty={!value} bind:this={opener} aria-label={`Edit ${label.toLowerCase()}: ${value || placeholder}`} {disabled} onclick={open}>
      <span class="text">{value || placeholder}</span>
      {#if !disabled}<span class="pencil" aria-hidden="true"><LucideIcon name="pencil" size={14} variant="action" /></span>{/if}
    </button>
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
</div>

<style>
  .inline-edit { display: grid; gap: 2px; min-width: 0; }
  .value { position: relative; display: flex; align-items: center; width: 100%; min-width: 0; min-height: 28px; padding: 4px 6px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); text-align: left; cursor: text; }
  .value:disabled { cursor: default; }
  .value:hover:not(:disabled), .value:focus-visible { background: var(--faint); }
  .strong .value, .strong input { font-size: var(--text-15); font-weight: 600; }
  .text { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty .text { color: var(--muted); }
  /* The pencil sits over the end of the text on the hover fill. */
  .pencil { position: absolute; top: 50%; right: 2px; display: none; place-items: center; width: 24px; height: 24px; transform: translateY(-50%); border-radius: var(--radius-control); background: var(--faint); color: var(--muted); }
  .value:hover:not(:disabled) .pencil, .value:focus-visible .pencil { display: grid; }
  input { box-sizing: border-box; width: 100%; min-width: 0; min-height: 28px; padding: 3px 5px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); outline: 0; }
  [role="alert"] { margin: 0 6px; color: var(--danger); font-size: var(--text-12); line-height: 1.4; overflow-wrap: anywhere; }
  @media (hover: none) { .value:not(:disabled) .pencil { display: grid; } }
</style>
