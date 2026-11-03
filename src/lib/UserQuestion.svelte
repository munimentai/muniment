<script>
  import PopupClose from './PopupClose.svelte'
  import { dialogDismiss } from './dialog-dismiss.js'
  import { panelScroll } from './panel-scroll.js'
  import { COMPOSER_PANEL_EVENT, openComposerPanel } from './composer-panels.js'
  import { onMount } from 'svelte'
  import { parseQuestions, pickOption, questionAnswerValue, questionsComplete } from './user-question.js'
  let { payload, anchor, pending = false, error = '', onanswer } = $props()
  let dialog
  let index = $state(0)
  let answers = $state({})
  let questions = $derived(parseQuestions(payload))
  let question = $derived(questions[index])
  let complete = $derived(questionsComplete(questions, answers))
  function update(id, change) { answers = { ...answers, [id]: { selected: [], text: '', ...answers[id], ...change } } }
  function select(label) { answers = pickOption(answers, question, label) }
  function open() { openComposerPanel('questions'); if (!dialog.open) dialog.showModal() }
  async function submit(event) {
    event.preventDefault()
    if (!complete || pending) return
    await onanswer(questionAnswerValue(questions, answers))
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
        // Stop below the title bar so the card never covers the window controls.
        const top = document.querySelector('.titlebar')?.getBoundingClientRect().bottom ?? 16
        dialog.style.maxHeight = `${Math.max(0, rect.top - top - 16)}px`
      }
      frame = requestAnimationFrame(position)
    }
    const dismiss = event => { if (!pending && event.detail !== 'questions') dialog.close() }
    window.addEventListener(COMPOSER_PANEL_EVENT, dismiss)
    open()
    position()
    return () => { cancelAnimationFrame(frame); window.removeEventListener(COMPOSER_PANEL_EVENT, dismiss) }
  })
</script>

<div class="question-waiting">
  <span>Waiting for your answers.</span>
  <button onclick={open}>Answer questions</button>
</div>
<dialog use:panelScroll data-panel="question" data-panel-variant="overlay" bind:this={dialog} aria-labelledby="question-title" use:dialogDismiss={{onclose: () => dialog.close(), disabled: pending, covers: false}}>
  <form onsubmit={submit}>
    <header><h2 id="question-title">A few details</h2><PopupClose disabled={pending} onclick={() => dialog.close()} /></header>
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
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { opacity: .55; cursor: default; }
  .question-waiting { color: var(--muted); font-size: var(--text-13); }
</style>
