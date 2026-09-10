<script>
  import { onMount } from 'svelte'

  const api = window.__TAURI__
  let input
  let text = $state('')
  let pending = $state(null)
  let ready = $state(false)
  let error = $state('')

  function showError(message) {
    error = message
    input?.setCustomValidity(message)
    input?.reportValidity()
  }

  async function close() {
    try {
      await api.core.invoke('launcher_close')
    } catch (_) {
      showError('The launcher could not close. Press Escape again.')
    }
  }

  async function send(event) {
    event.preventDefault()
    if (event.isComposing || pending || !ready || !text.trim()) return
    pending = crypto.randomUUID()
    error = ''
    input.setCustomValidity('')
    try {
      await api.event.emitTo('main', 'launcher-submit', { id: pending, text: text.trim() })
    } catch (_) {
      pending = null
      showError('The message did not send. Press Enter to retry.')
    }
  }

  function keydown(event) {
    if (event.isComposing) return
    if (event.key === 'Escape') {
      event.preventDefault()
      void close()
    }
    if (event.key === 'Enter') void send(event)
  }

  onMount(() => {
    let destroyed = false
    const stops = []
    async function start() {
      for (const [name, handler] of [
        ['launcher-opened', () => input?.focus()],
        ['launcher-result', async ({ payload }) => {
          if (!pending || payload?.id !== pending) return
          pending = null
          if (payload.error) {
            showError(payload.error)
            return
          }
          text = ''
          try {
            await api.core.invoke('launcher_present_main')
          } catch (_) {
            showError('The message sent. Open the main window to see it.')
          }
        }],
      ]) {
        const stop = await api.event.listen(name, handler)
        if (destroyed) { stop(); return }
        stops.push(stop)
      }
      ready = true
      input.focus()
    }
    void start().catch(() => showError('The launcher could not start. Restart the app.'))
    return () => { destroyed = true; stops.forEach((stop) => stop()) }
  })
</script>

<svelte:window onkeydown={keydown} />

<form class="launcher" aria-label="New thread" onsubmit={send}>
  <input
    bind:this={input}
    bind:value={text}
    aria-label="First message"
    aria-describedby={error ? 'launcher-error' : undefined}
    aria-invalid={!!error}
    aria-busy={!!pending}
    placeholder="Ask anything"
    autocomplete="off"
    spellcheck="true"
    readonly={!!pending}
    oninput={() => { error = ''; input.setCustomValidity('') }}
  />
  <span id="launcher-error" class="visually-hidden" role="alert">{error}</span>
</form>

<style>
  .launcher {
    width: 100vw;
    height: 100vh;
    display: flex;
    align-items: center;
    padding: 0 24px;
    background: var(--surface);
    border: 1px solid var(--border);
  }
  .launcher:focus-within { border-color: var(--muted); }
  input {
    width: 100%;
    min-width: 0;
    height: 48px;
    padding: 0;
    border: 0;
    outline: none;
    background: transparent;
    color: var(--ink);
    font: inherit;
    font-size: var(--text-17);
  }
  input::placeholder { color: var(--muted); opacity: 1; }
</style>
