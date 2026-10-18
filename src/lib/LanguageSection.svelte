<script>
  // Settings → Language: the language the app and its Browser tab use.
  import BrowserLanguage from './BrowserLanguage.svelte'
  import PageHead from './ui/PageHead.svelte'
  let { tauri } = $props()
</script>

<div class="language-preferences">
  <PageHead intro="The language of the Browser tab's pages and site requests." />
  <BrowserLanguage {tauri} />
</div>

<style>
  .language-preferences { display: grid; gap: var(--page-gap); }
</style>
