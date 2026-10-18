<script>
  // When a run raises a system notification. Both choices are fleet
  // preferences in core, so every machine of the user notifies the same way.
  import PageHead from '../lib/ui/PageHead.svelte'
  import Group from '../lib/ui/Group.svelte'
  import Row from '../lib/ui/Row.svelte'
  import Toggle from '../lib/Toggle.svelte'
  import { DEFAULT_NOTIFY, readNotifyPreferences, saveNotifyPreferences } from './notifications.js'

  let { tauri } = $props()
  let preferences = $state({ ...DEFAULT_NOTIFY })
  let error = $state('')
  $effect(() => { void readNotifyPreferences(tauri).then((value) => { preferences = value }) })

  async function save(key, value) {
    const previous = preferences
    preferences = { ...preferences, [key]: value }
    try {
      preferences = await saveNotifyPreferences(tauri, preferences)
      window.dispatchEvent(new CustomEvent('muniment:notify-preferences', { detail: preferences }))
      error = ''
    } catch (reason) {
      preferences = previous
      error = String(reason?.message ?? reason)
    }
  }
</script>

<PageHead intro="System notifications for agent runs this app did not start." />
<Group heading="System notifications">
  <Row title="Needs you" description="An approval, a question, a failure or a usage limit.">
    <Toggle label="Notify when a run needs you" checked={preferences.needs_you} onchange={(value) => save('needs_you', value)} />
  </Row>
  <Row title="Finished" description="A run that finished with a reply.">
    <Toggle label="Notify when a run finishes" checked={preferences.finished} onchange={(value) => save('finished', value)} />
  </Row>
</Group>
{#if error}<p class="error" role="alert">{error}</p>{/if}

<style>
  .error { margin: 0; color: var(--danger); font-size: var(--text-13); }
</style>
