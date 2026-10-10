<script>
  // Settings → Voice: the system-wide dictation shortcut, the microphone, the
  // on-device speech model and how a transcript lands. The shortcut and the
  // speech model run today. The rest is a design ahead of the runtime, and its
  // choices stay on this device until dictation reads them.
  import { onDestroy, onMount } from 'svelte'
  import ChoiceField from './ui/ChoiceField.svelte'
  import Toggle from './Toggle.svelte'
  import PageHead from './ui/PageHead.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Button from './ui/Button.svelte'
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
  <PageHead intro="Dictate into any chat. Speech stays on this device." />

  <Group heading="Shortcut">
    <Row title="Dictation shortcut" description="Works from anywhere, even when Muniment is in the background.">
      <button class="shortcut-capture" aria-label={capturingShortcut ? 'Record voice shortcut' : `Change voice shortcut, current ${voiceShortcut}`} onclick={() => { capturingShortcut = true; pendingShortcut = ''; shortcutStatus = '' }} onkeydown={captureShortcut} disabled={voiceShortcutChanging}>
        <span>{capturingShortcut ? pendingShortcut || 'Press a shortcut…' : voiceShortcut}</span><small>{capturingShortcut ? 'Modifier + key' : 'Change'}</small>
      </button>
      {#if capturingShortcut}
        <Button onclick={() => { capturingShortcut = false; pendingShortcut = ''; shortcutStatus = '' }}>Cancel</Button>
        <Button onclick={() => applyShortcut(pendingShortcut)} disabled={!pendingShortcut || voiceShortcutChanging}>{voiceShortcutChanging ? 'Applying…' : 'Apply'}</Button>
      {/if}
      <button class="restore-shortcut" onclick={() => applyShortcut(defaultVoiceShortcut)} disabled={voiceShortcut === defaultVoiceShortcut || voiceShortcutChanging}>Restore default</button>
    </Row>
    {#if shortcutStatus}<p class="shortcut-error" role="alert">{shortcutStatus}</p>{/if}
    <Row title="Shortcut behavior" description="How a press starts and ends dictation.">
      <div class="choice"><ChoiceField label="Shortcut behavior" hideLabel value={voice.behavior} options={behaviors} onchange={(behavior) => remember({ behavior })} /></div>
    </Row>
    <Row title="Cancel dictation" description="Stops without inserting the words."><kbd>Escape</kbd></Row>
  </Group>

  <Group heading="Microphone">
    <Row title="Input" description="The microphone dictation listens to.">
      <div class="choice"><ChoiceField label="Microphone" hideLabel value={voice.microphone} options={microphones} onchange={(microphone) => remember({ microphone })} /></div>
    </Row>
    <Row title="Mute other audio while dictating" description="Music and calls pause while you speak.">
      <Toggle checked={voice.mute} label="Mute other audio while dictating" onchange={(mute) => remember({ mute })} />
    </Row>
    <Row title="Start and stop sounds" description="A short tone marks each end of a dictation.">
      <Toggle checked={voice.sound} label="Start and stop sounds" onchange={(sound) => remember({ sound })} />
    </Row>
  </Group>

  <Group heading="Speech model">
    <Row title="Parakeet" meta={install?.state === 'installed' ? 'Active' : 'On this device'} description="Fast, accurate English dictation, with a voice activity model that hears when you stop.">
      {#snippet detail()}
        <dl>
          <div><dt>Download</dt><dd>{facts ? formatBytes(facts.totalDownloadBytes) : 'Unknown size'}</dd></div>
          <div><dt>Free disk required</dt><dd>{facts ? formatBytes(facts.requiredFreeBytes) : 'Unknown size'}</dd></div>
          <div><dt>State</dt><dd role="status">{install ? installStateWords(install.state) : 'Install state unavailable.'}</dd></div>
        </dl>
      {/snippet}
      {#if install?.state === 'installing'}
        <Button onclick={cancelInstall}>Cancel install</Button>
      {:else if install?.state !== 'installed'}
        <Button onclick={startInstall}>Install</Button>
      {/if}
    </Row>
    {#if installError}<p role="alert">{installError}</p>{/if}
  </Group>

  <Group heading="Transcription">
    <Row title="Remove filler words" description="Drops um, uh and false starts from the transcript.">
      <Toggle checked={voice.fillers} label="Remove filler words" onchange={(fillers) => remember({ fillers })} />
    </Row>
    <Row title="Send when dictation stops" description="The chat sends the transcript without a press of Enter.">
      <Toggle checked={voice.send} label="Send when dictation stops" onchange={(send) => remember({ send })} />
    </Row>
    <Row title="Custom words" description="Names and terms the model should spell your way.">
      {#snippet detail()}
        {#if voice.words.length}
          <ul class="word-list" aria-label="Custom words">
            {#each voice.words as entry (entry)}
              <li><span>{entry}</span><button type="button" aria-label={`Remove ${entry}`} onclick={() => remember({ words: voice.words.filter((item) => item !== entry) })}>×</button></li>
            {/each}
          </ul>
        {/if}
      {/snippet}
      <form onsubmit={addWord}><input aria-label="Custom word" placeholder="Add a word" bind:value={word} /><Button type="submit" disabled={!word.trim()}>Add</Button></form>
    </Row>
  </Group>
</div>

<style>
  /* Voice builds from PageHead, Group and Row like every Settings page. */
  .voice { display: grid; gap: var(--page-gap); }
  .shortcut-capture { display: inline-flex; align-items: center; justify-content: space-between; gap: 12px; min-width: 200px; min-height: 32px; box-sizing: border-box; padding: 6px 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-mono); cursor: pointer; }
  .shortcut-capture:hover:not(:disabled) { border-color: var(--muted); }
  .shortcut-capture small { color: var(--muted); font: var(--text-12) var(--font-human); }
  .restore-shortcut { min-height: 32px; padding: 6px 10px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: var(--text-13) var(--font-human); cursor: pointer; }
  .restore-shortcut:hover:not(:disabled) { background: var(--faint); color: var(--ink); }
  .restore-shortcut:disabled { opacity: .5; cursor: default; }
  .shortcut-error { color: var(--oxide) !important; }
  kbd { padding: 3px 8px; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--faint); font: var(--text-12) var(--font-mono); }
  dl { display: flex; flex-wrap: wrap; gap: 4px 24px; margin: 6px 0 0; }
  dt { color: var(--muted); font-size: var(--text-12); }
  dd { margin: 0; font: var(--text-12) var(--font-mono); }
  .choice { width: 280px; max-width: 100%; }
  form { display: flex; gap: 6px; }
  input { width: 180px; height: 32px; box-sizing: border-box; padding: 0 10px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-13) var(--font-human); }
  input:focus { outline: none; border-color: var(--muted); }
  .word-list { display: flex; flex-wrap: wrap; gap: 6px; margin: 6px 0 0; padding: 0; list-style: none; }
  .word-list li { display: inline-flex; align-items: center; gap: 2px; padding: 0 0 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-12) var(--font-mono); }
  .word-list button { min-width: 24px; min-height: 24px; padding: 0 6px; border: 0; border-radius: var(--radius-control); background: transparent; color: var(--muted); cursor: pointer; }
  .word-list button:hover { background: var(--faint); color: var(--ink); }
</style>
