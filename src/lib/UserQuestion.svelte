<script>
  import { panelScroll } from './panel-scroll.js'
  import { COMPOSER_PANEL_EVENT, openComposerPanel } from './composer-panels.js'
  import { onMount } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  let { payload, anchor, pending = false, error = '', onanswer } = $props()
  let dialog
  let index = $state(0)
  let answers = $state({})
  let questions = $derived.by(() => {
    try {
      const value = JSON.parse(payload).questions
      if (!Array.isArray(value) || value.length < 1 || value.length > 4) return []
      if (new Set(value.map(q => q.id)).size !== value.length) return []
      return value.every(q => typeof q.id === 'string' && typeof q.question === 'string'
        && (!q.options || (Array.isArray(q.options) && q.options.length <= 6 && q.options.every(o => typeof o.label === 'string')))) ? value : []
    } catch { return [] }
  })
  let question = $derived(questions[index])
  let complete = $derived(questions.length > 0 && questions.every(q => answers[q.id]?.selected?.length || answers[q.id]?.text?.trim()))
  function update(id, change) { answers = { ...answers, [id]: { selected: [], text: '', ...answers[id], ...change } } }
  function select(label) {
    const selected = answers[question.id]?.selected ?? []
    update(question.id, { selected: question.multiSelect ? (selected.includes(label) ? selected.filter(x => x !== label) : [...selected, label]) : [label] })
  }
  function open() { openComposerPanel('questions'); if (!dialog.open) dialog.showModal() }
  async function submit(event) {
    event.preventDefault()
    if (!complete || pending) return
    await onanswer(JSON.stringify({ answers: questions.map(q => ({ id: q.id, question: q.question, ...answers[q.id] })) }))
  }
  onMount(() => {
    let frame
    // The dialog is in the top layer. Follow the composer through panel resizing
    // and text growth rather than positioning against the transcript.
    function position() {
      if (dialog.open && anchor) {
        const rect = anchor.getBoundingClientRect()
        dialog.style.left = `${rect.left}px`
        dialog.style.width = `${rect.width}px`
        dialog.style.bottom = `${window.innerHeight - rect.top + 8}px`
        dialog.style.maxHeight = `${Math.max(0, rect.top - 24)}px`
      }
      frame = requestAnimationFrame(position)
    }
    const dismiss = event => { if (event.detail !== 'questions') dialog.close() }
    const outside = event => {
      if (event.target !== dialog) return
      const rect = dialog.getBoundingClientRect()
      if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) dialog.close()
    }
    window.addEventListener(COMPOSER_PANEL_EVENT, dismiss)
    dialog.addEventListener('pointerdown', outside)
    open()
    position()
    return () => { cancelAnimationFrame(frame); window.removeEventListener(COMPOSER_PANEL_EVENT, dismiss); dialog.removeEventListener('pointerdown', outside) }
  })
</script>

<div class="question-waiting">
  <span>Waiting for your answers.</span>
  <button onclick={open}>Answer questions</button>
</div>
<dialog use:panelScroll data-panel="question" data-panel-variant="overlay" bind:this={dialog} aria-labelledby="question-title" oncancel={(event) => { event.preventDefault(); if (!pending) dialog.close() }}>
  <form onsubmit={submit}>
    <header><h2 id="question-title">A few details</h2><button type="button" class="close" aria-label="Close" disabled={pending} onclick={() => dialog.close()}><LucideIcon name="x" variant="action" size={16} /></button></header>
    {#if question}
      <p class="progress">Question {index + 1} of {questions.length}</p>
      <fieldset disabled={pending}>
        <legend>{question.question}</legend>
        {#each question.options ?? [] as option}
          <label class="option">
            <input type={question.multiSelect ? 'checkbox' : 'radio'} name={question.id} checked={answers[question.id]?.selected?.includes(option.label) ?? false} onchange={() => select(option.label)} />
            <span>{option.label}{#if option.description}<small>{option.description}</small>{/if}</span>
          </label>
        {/each}
        <label class="custom">Your answer
          <textarea rows="3" value={answers[question.id]?.text ?? ''} oninput={(event) => update(question.id, { text: event.currentTarget.value })} placeholder="Write an answer or add details"></textarea>
        </label>
      </fieldset>
      <footer>
        <button type="button" disabled={index === 0 || pending} onclick={() => index--}>Back</button>
        {#if index < questions.length - 1}
          <button type="button" disabled={pending} onclick={() => index++}>Next</button>
        {:else}
          <button type="submit" disabled={!complete || pending}>{pending ? 'Submitting…' : 'Submit answers'}</button>
        {/if}
      </footer>
    {:else}<p role="alert">The questions could not be displayed. Stop the reply and try again.</p>{/if}
    {#if error}<p role="alert">{error}</p>{/if}
  </form>
</dialog>

<style>
  dialog { position: fixed; inset: auto auto 24px 24px; margin: 0; width: calc(100vw - 48px); max-height: calc(100vh - 140px); box-sizing: border-box; overflow: auto; padding: 24px;      font-family: var(--font-human); }
  dialog::backdrop { background: transparent; }
  header, footer, .question-waiting { display: flex; align-items: center; gap: 12px; }
  header { justify-content: space-between; }
  footer { justify-content: flex-end; margin-top: 20px; }
  h2 { margin: 0; font-size: var(--text-17); }
  .progress, small { color: var(--muted); font-size: var(--text-13); }
  fieldset { padding: 0; border: 0; margin: 0; min-width: 0; }
  legend { margin-bottom: 16px; font-size: var(--text-15); overflow-wrap: anywhere; }
  .option { display: flex; align-items: start; gap: 10px; border: 1px solid var(--border); border-radius: var(--radius-control); padding: 12px; margin-bottom: 8px; cursor: pointer; overflow-wrap: anywhere; }
  small { display: block; margin-top: 4px; }
  input { margin-top: 4px; }
  .custom { display: block; margin-top: 16px; font-size: var(--text-13); }
  textarea { display: block; box-sizing: border-box; width: 100%; resize: vertical; margin-top: 8px; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--paper); color: var(--ink); font: inherit; }
  button { min-height: 36px; padding: 7px 14px; border: 1px solid var(--border); border-radius: var(--radius-control); background: transparent; color: var(--ink); font: inherit; cursor: pointer; }
  button.close { display: grid; place-items: center; flex: none; min-height: 28px; width: 28px; padding: 0; border: 0; color: var(--muted); }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { opacity: .55; cursor: default; }
  .question-waiting { color: var(--muted); font-size: var(--text-13); }
</style>
