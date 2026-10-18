<script>
  // Settings → General → Language: one language for the app and its Browser tab.
  import ChoiceField from './ui/ChoiceField.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import { onMount } from 'svelte'
  let { tauri } = $props()
  let view = $state(null)
  let pending = $state(false)
  let downloading = $state(false)
  let error = $state('')

  // Each language shows its own name, then its English name when they differ.
  function languageName(code) {
    const name = (locale) => { try { return new Intl.DisplayNames([locale], { type: 'language' }).of(code) } catch { return code } }
    const own = name(code), english = name('en')
    return own === english ? own : `${own} (${english})`
  }
  const options = $derived([
    { value: 'system', label: 'Match system' },
    ...(view?.languages ?? []).map(({ code, state }) => ({ value: code, label: state === 'available' ? `${languageName(code)} · Download` : languageName(code) })),
  ])
  const megabytes = $derived(Math.max(1, Math.round((view?.downloadSize ?? 0) / 1e6)))
  const restart = $derived(view && (view.selected ?? null) !== (view.active ?? null))

  onMount(async () => {
    try { view = await tauri.invoke('browser_language') } catch { error = 'The language could not be read.' }
  })
  async function choose(value) {
    pending = true; error = ''
    downloading = view?.languages.some(language => language.code === value && language.state === 'available')
    try { view = await tauri.invoke('browser_language_set', { code: value === 'system' ? null : value }) }
    catch (failure) { error = String(failure?.message ?? failure) }
    finally { pending = false; downloading = false }
  }
</script>
<Group heading="Browser language">
  <Row title="Language" description={view?.languages.some(language => language.state === 'available') ? `Download fetches about ${megabytes} MB once` : ''}>
    {#if view}<div class="choice"><ChoiceField label="Language" hideLabel value={view.selected ?? 'system'} {options} disabled={pending} onchange={choose} /></div>{/if}
  </Row>
  {#if downloading}<p role="status">Downloading the language…</p>{/if}
  {#if restart && !pending}<p role="status">Restart Muniment to change the language.</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</Group>
<style>
  .choice { width: 280px; max-width: 100%; }
</style>
