<script>
  // Settings → Voice: the system-wide dictation shortcut, the microphone, the
  // on-device speech model and how a transcript lands. The shortcut and the
  // speech model run today. The rest is a design ahead of the runtime, and its
  // choices stay on this device until dictation reads them.
  import { onDestroy, onMount } from 'svelte'
  import ChoiceField from './ui/ChoiceField.svelte'
  import Toggle from './Toggle.svelte'
  import { shortcutFromKeyboardEvent } from './dictation-state.js'
  import { formatBytes, installStateWords } from './speech-install.js'
  import { readDevicePreference, saveDevicePreference } from './device-preferences.js'

  let { tauri, voiceShortcut = '', voiceShortcutChanging = false, onVoiceShortcutChange = async () => true, defaultVoiceShortcut = '' } = $props()

  // The shortcut: capture a modifier plus a key, apply it, or restore the default.
  let capturingShortcut = $state(false)
  let pendingShortcut = $state('')
  let shortcutStatus = $state('')

  function captureShortcut(event) {
    if (!capturingShortcut) return
    event.preventDefault()
    event.stopPropagation()
    if (event.key === 'Escape') {
      capturingShortcut = false
      pendingShortcut = ''
      return
    }
    const shortcut = shortcutFromKeyboardEvent(event)
    if (shortcut) {
      pendingShortcut = shortcut
      shortcutStatus = ''
    } else if (!['Alt', 'Control', 'Meta', 'Shift'].includes(event.key)) shortcutStatus = 'Include at least one modifier key.'
  }

  async function applyShortcut(shortcut) {
    shortcutStatus = ''
    const result = await onVoiceShortcutChange(shortcut)
    if (result) {
      capturingShortcut = false
      pendingShortcut = ''
    } else if (result === null) shortcutStatus = 'The shortcut could not be updated. Voice remains available from the button.'
    else shortcutStatus = 'That shortcut is unavailable. Your previous shortcut still works.'
  }

  // Choices ahead of the runtime, kept on this device.
  let voice = $state(readDevicePreference('muniment.voice', { behavior: 'auto', microphone: 'default', mute: true, sound: false, fillers: true, words: [], send: false }))
  const remember = (next) => { voice = saveDevicePreference('muniment.voice', { ...voice, ...next }) }
  const behaviors = [
    { value: 'auto', label: 'Hold, or press twice for hands-free' },
    { value: 'hold', label: 'Hold to talk' },
    { value: 'toggle', label: 'Press to start and stop' },
  ]
  let microphones = $state([{ value: 'default', label: 'System default' }])
  let word = $state('')
  function addWord(event) {
    event.preventDefault()
    const next = word.trim()
    if (next && !voice.words.includes(next)) remember({ words: [...voice.words, next] })
    word = ''
  }

  // The speech model: Parakeet and its voice activity model, installed on demand.
  let facts = $state(null)
  let install = $state(null)
  let installError = $state('')
  let poll
  async function readInstall() {
    try { install = await tauri.invoke('parakeet_install_status') } catch (_) { install = null }
    clearTimeout(poll)
    if (install?.state === 'installing') poll = setTimeout(readInstall, 1000)
  }
  async function startInstall() {
    installError = ''
    try { await tauri.invoke('parakeet_install_start'); await readInstall() } catch (failure) { installError = String(failure?.message ?? failure) }
  }
  async function cancelInstall() {
    try { await tauri.invoke('parakeet_install_cancel') } catch (_) {}
    await readInstall()
  }

  onMount(async () => {
    try { facts = await tauri.invoke('parakeet_install_facts') } catch (_) { facts = null }
    await readInstall()
    try {
      const devices = await navigator.mediaDevices?.enumerateDevices?.() ?? []
      const inputs = devices.filter((device) => device.kind === 'audioinput' && device.deviceId !== 'default' && device.label)
      if (inputs.length) microphones = [microphones[0], ...inputs.map((device) => ({ value: device.deviceId, label: device.label }))]
    } catch (_) {}
  })
  onDestroy(() => clearTimeout(poll))
</script>

<div class="voice">
  <p class="settings-intro">Dictate into any chat. Speech stays on this device.</p>

  <section class="voice-block" aria-labelledby="voice-shortcut-heading">
    <h3 id="voice-shortcut-heading" class="settings-heading">Shortcut</h3>
    <div class="rows">
      <div class="row">
        <div><strong>Dictation shortcut</strong><p>Works from anywhere, even when Muniment is in the background.</p></div>
        <div class="shortcut">
          <button class="shortcut-capture" aria-label={capturingShortcut ? 'Record voice shortcut' : `Change voice shortcut, current ${voiceShortcut}`} onclick={() => { capturingShortcut = true; pendingShortcut = ''; shortcutStatus = '' }} onkeydown={captureShortcut} disabled={voiceShortcutChanging}>
            <span>{capturingShortcut ? pendingShortcut || 'Press a shortcut…' : voiceShortcut}</span><small>{capturingShortcut ? 'Modifier + key' : 'Change'}</small>
          </button>
          {#if capturingShortcut}
            <button onclick={() => { capturingShortcut = false; pendingShortcut = ''; shortcutStatus = '' }}>Cancel</button>
            <button onclick={() => applyShortcut(pendingShortcut)} disabled={!pendingShortcut || voiceShortcutChanging}>{voiceShortcutChanging ? 'Applying…' : 'Apply'}</button>
          {/if}
          <button class="quiet restore-shortcut" onclick={() => applyShortcut(defaultVoiceShortcut)} disabled={voiceShortcut === defaultVoiceShortcut || voiceShortcutChanging}>Restore default</button>
        </div>
      </div>
      {#if shortcutStatus}<p class="shortcut-error" role="alert">{shortcutStatus}</p>{/if}
      <div class="row">
        <div><strong>Shortcut behavior</strong><p>How a press starts and ends dictation.</p></div>
        <div class="choice"><ChoiceField label="Shortcut behavior" value={voice.behavior} options={behaviors} onchange={(behavior) => remember({ behavior })} /></div>
      </div>
      <div class="row">
        <div><strong>Cancel dictation</strong><p>Stops without inserting the words.</p></div>
        <kbd>Escape</kbd>
      </div>
    </div>
  </section>

  <section class="voice-block" aria-labelledby="voice-sound-heading">
    <h3 id="voice-sound-heading" class="settings-heading">Microphone</h3>
    <div class="rows">
      <div class="row">
        <div><strong>Input</strong><p>The microphone dictation listens to.</p></div>
        <div class="choice"><ChoiceField label="Microphone" value={voice.microphone} options={microphones} onchange={(microphone) => remember({ microphone })} /></div>
      </div>
      <div class="row">
        <div><strong>Mute other audio while dictating</strong><p>Music and calls pause while you speak.</p></div>
        <Toggle checked={voice.mute} label="Mute other audio while dictating" onchange={(mute) => remember({ mute })} />
      </div>
      <div class="row">
        <div><strong>Start and stop sounds</strong><p>A short tone marks each end of a dictation.</p></div>
        <Toggle checked={voice.sound} label="Start and stop sounds" onchange={(sound) => remember({ sound })} />
      </div>
    </div>
  </section>

  <section class="voice-block" aria-labelledby="voice-model-heading">
    <h3 id="voice-model-heading" class="settings-heading">Speech model</h3>
    <div class="model" class:installed={install?.state === 'installed'}>
      <div class="model-head">
        <strong>Parakeet</strong>
        <span class="chip">{install?.state === 'installed' ? 'Active' : 'On this device'}</span>
      </div>
      <p>Fast, accurate English dictation, with a voice activity model that hears when you stop.</p>
      <dl>
        <div><dt>Download</dt><dd>{facts ? formatBytes(facts.totalDownloadBytes) : 'Unknown size'}</dd></div>
        <div><dt>Free disk required</dt><dd>{facts ? formatBytes(facts.requiredFreeBytes) : 'Unknown size'}</dd></div>
        <div><dt>State</dt><dd role="status">{install ? installStateWords(install.state) : 'Install state unavailable.'}</dd></div>
      </dl>
      {#if install?.state === 'installing'}
        <button type="button" onclick={cancelInstall}>Cancel install</button>
      {:else if install?.state !== 'installed'}
        <button type="button" onclick={startInstall}>Install</button>
      {/if}
      {#if installError}<p role="alert">{installError}</p>{/if}
    </div>
  </section>

  <section class="voice-block" aria-labelledby="voice-text-heading">
    <h3 id="voice-text-heading" class="settings-heading">Transcription</h3>
    <div class="rows">
      <div class="row">
        <div><strong>Remove filler words</strong><p>Drops um, uh and false starts from the transcript.</p></div>
        <Toggle checked={voice.fillers} label="Remove filler words" onchange={(fillers) => remember({ fillers })} />
      </div>
      <div class="row">
        <div><strong>Send when dictation stops</strong><p>The chat sends the transcript without a press of Enter.</p></div>
        <Toggle checked={voice.send} label="Send when dictation stops" onchange={(send) => remember({ send })} />
      </div>
      <div class="row words">
        <div><strong>Custom words</strong><p>Names and terms the model should spell your way.</p></div>
        <form onsubmit={addWord}><input aria-label="Custom word" placeholder="Add a word" bind:value={word} /><button type="submit" disabled={!word.trim()}>Add</button></form>
      </div>
      {#if voice.words.length}
        <ul class="word-list" aria-label="Custom words">
          {#each voice.words as entry (entry)}
            <li><span>{entry}</span><button type="button" class="quiet" aria-label={`Remove ${entry}`} onclick={() => remember({ words: voice.words.filter((item) => item !== entry) })}>×</button></li>
          {/each}
        </ul>
      {/if}
    </div>
  </section>
</div>

<style>
  /* Voice reads like every Settings page: the intro, then sections under
     semibold headings with their rows in bordered groups. */
  .voice { display: grid; gap: 28px; max-width: 760px; }
  .voice > .settings-intro { margin: 0 0 -14px; }
  .voice-block { display: grid; gap: 10px; }
  .voice-block > h3 { margin: 0; }
  .rows { display: grid; padding: 0 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 16px; min-height: 44px; padding: 10px 0; }
  .row + .row, .rows > p + .row { border-top: 1px solid var(--border); }
  .row > div:first-child { display: grid; gap: 2px; min-width: 0; }
  strong { font-size: var(--text-13); font-weight: 600; }
  p { margin: 0; color: var(--muted); font-size: var(--text-13); }
  button { flex: none; font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--faint); border: 1px solid transparent; border-radius: var(--radius-control); padding: 6px 10px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--border); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; }
  .shortcut { display: flex; align-items: center; gap: 6px; flex: none; }
  .shortcut-capture { display: inline-flex; align-items: center; gap: 12px; min-width: 200px; justify-content: space-between; padding: 6px 10px; border-color: var(--border); background: var(--surface); font-family: var(--font-mono); }
  .shortcut-capture small { color: var(--muted); font: var(--text-12) var(--font-human); }
  .restore-shortcut { color: var(--muted); }
  .shortcut-error { padding: 0 0 10px; color: var(--oxide); }
  kbd { flex: none; padding: 3px 8px; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--faint); font: var(--text-12) var(--font-mono); }
  .model { display: grid; gap: 8px; padding: 14px; border: 1px solid var(--border); border-radius: var(--radius-panel); justify-items: start; }
  .model.installed { background: var(--faint); }
  .model-head { display: flex; align-items: center; gap: 8px; }
  .model-head strong { font-size: var(--text-15); }
  .chip { padding: 1px 6px; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--surface); color: var(--muted); font: var(--text-12) var(--font-mono); }
  dl { display: flex; flex-wrap: wrap; gap: 4px 24px; margin: 0; }
  dt { color: var(--muted); font-size: var(--text-12); }
  dd { margin: 0; font: var(--text-12) var(--font-mono); }
  form { display: flex; gap: 6px; flex: none; }
  .words { flex-wrap: wrap; }
  /* A row's title names its choice, so the field keeps only its accessible name. */
  .choice { flex: none; width: 280px; }
  .choice :global(.field > span:first-child) { display: none; }
  input { width: 180px; height: 32px; box-sizing: border-box; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  input:focus { outline: none; border-color: var(--muted); }
  .word-list { display: flex; flex-wrap: wrap; gap: 6px; margin: 0; padding: 0 0 12px; list-style: none; }
  .word-list li { display: inline-flex; align-items: center; gap: 2px; padding: 0 0 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-12) var(--font-mono); }
  .word-list button { min-width: 24px; min-height: 24px; padding: 0 6px; }
</style>
