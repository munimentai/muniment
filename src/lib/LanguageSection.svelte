<script>
  // Settings → Language: the language the app and its Browser tab use.
  import BrowserLanguage from './BrowserLanguage.svelte'
  import PageHead from './ui/PageHead.svelte'
  let { tauri } = $props()
</script>

<div class="language-preferences">
  <PageHead intro="Choose the language the Browser tab uses." />
  <BrowserLanguage {tauri} />
</div>

<style>
  .language-preferences { display: grid; gap: var(--page-gap); }
</style>
