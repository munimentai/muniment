<script>
  // Settings → Language: the language the app and its Browser tab use, then the engine the address bar searches.
  import BrowserLanguage from './BrowserLanguage.svelte'
  import BrowserSearch from './BrowserSearch.svelte'
  import PageHead from './ui/PageHead.svelte'
  let { tauri } = $props()
</script>

<div class="language-preferences">
  <PageHead intro="Choose the language the Browser tab uses, and the engine the address bar searches." />
  <BrowserLanguage {tauri} />
  <BrowserSearch />
</div>

<style>
  .language-preferences { display: grid; gap: var(--page-gap); }
</style>
