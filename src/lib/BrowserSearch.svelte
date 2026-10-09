<script>
  import ChoiceField from './ui/ChoiceField.svelte'
  import { SEARCH_ENGINES, readSearchEngine, saveSearchEngine } from './browser-settings.js'
  let engine = $state(readSearchEngine())
  let error = $state('')
  const options = SEARCH_ENGINES.map(({ id, name }) => ({ value: id, label: name }))
  function choose(value) {
    try { engine = saveSearchEngine(value); error = '' }
    catch (failure) { error = String(failure?.message ?? failure) }
  }
</script>
<section aria-labelledby="browser-search-heading">
  <h3 id="browser-search-heading">Browser search</h3>
  <p>Words in the Browser tab address bar search with this engine.</p>
  <ChoiceField label="Search engine" value={engine} {options} onchange={choose} />
  <p>Add a bang before or after the words to search one site, such as !w for Wikipedia, !yt for YouTube or !gh for GitHub. DuckDuckGo answers other bangs.</p>
  {#if error}<p role="alert">{error}</p>{/if}
</section>
<style>
  section { display: grid; gap: 10px; align-content: start; }
  h3 { margin: 0; font: var(--text-13) var(--font-mono); color: var(--muted); }
  p { margin: 0; font-size: var(--text-13); color: var(--muted); }
</style>
