<script>
  import { onMount } from 'svelte'
  import { THEME_STORAGE_KEY, applyTheme, readStoredTheme } from './lib/theme-state.js'

  const api = window.__TAURI__
  let input
  let text = $state('')
  let pending = $state(null)
  let ready = $state(false)
  let error = $state('')

  function syncAppearance() {
    applyTheme(document.documentElement, readStoredTheme())
  }

  function storageChanged(event) {
    if (event.storageArea === localStorage && (event.key === THEME_STORAGE_KEY || event.key === null)) {
      syncAppearance()
    }
  }

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
        ['launcher-opened', () => { syncAppearance(); input?.focus() }],
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
      syncAppearance()
      input.focus()
    }
    void start().catch(async (failure) => {
      if (destroyed) return
      ready = false
      void Promise.allSettled(stops.splice(0).map((stop) => Promise.resolve().then(stop)))
      const cause = failure instanceof Error ? failure.message : String(failure)
      const message = `The launcher could not start: ${cause}. Restart the app.`
      showError(message)
      console.error(message)
      try {
        await api.core.invoke('launcher_start_failed', { cause })
      } catch (error) {
        console.error('The launcher could not log its start failure.', error)
      }
    })
    return () => { destroyed = true; stops.forEach((stop) => stop()) }
  })
</script>

<svelte:window onkeydown={keydown} onstorage={storageChanged} />

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
