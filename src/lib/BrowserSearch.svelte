<script>
  import ChoiceField from './ui/ChoiceField.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import { SEARCH_ENGINES, readSearchEngine, saveSearchEngine } from './browser-settings.js'
  let engine = $state(readSearchEngine())
  let error = $state('')
  const options = SEARCH_ENGINES.map(({ id, name }) => ({ value: id, label: name }))
  function choose(value) {
    try { engine = saveSearchEngine(value); error = '' }
    catch (failure) { error = String(failure?.message ?? failure) }
  }
</script>
<Group heading="Browser search" description="Words in the Browser tab address bar search with this engine.">
  <Row title="Search engine" description="Add a bang before or after the words to search one site, such as !w for Wikipedia, !yt for YouTube or !gh for GitHub. DuckDuckGo answers other bangs.">
    <div class="choice"><ChoiceField label="Search engine" hideLabel value={engine} {options} onchange={choose} /></div>
  </Row>
  {#if error}<p role="alert">{error}</p>{/if}
</Group>
<style>
  .choice { width: 280px; max-width: 100%; }
</style>
