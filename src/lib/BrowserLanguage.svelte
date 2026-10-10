<script>
  // Settings → Language: one language for the app and its Browser tab.
  import ChoiceField from './ui/ChoiceField.svelte'
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
<section aria-labelledby="app-language-heading">
  <h3 id="app-language-heading" class="settings-heading">App language</h3>
  <p>Muniment's menus, messages and settings use this language, and so do the Browser tab's pages and site requests.</p>
  {#if view}
    <ChoiceField label="Language" value={view.selected ?? 'system'} {options} disabled={pending} onchange={choose} />
    {#if view.languages.some(language => language.state === 'available')}<p>Languages marked Download fetch about {megabytes} MB once.</p>{/if}
  {/if}
  {#if downloading}<p role="status">Downloading the language…</p>{/if}
  {#if restart && !pending}<p role="status">Restart Muniment to change the language.</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  section { display: grid; gap: 10px; align-content: start; }
  h3 { margin: 0; font: var(--text-13) var(--font-mono); color: var(--muted); }
  p { margin: 0; font-size: var(--text-13); color: var(--muted); }
</style>
