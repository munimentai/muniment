<script>
  // Settings → Language: the language the app and its Browser tab use, then the engine the address bar searches.
  import BrowserLanguage from './BrowserLanguage.svelte'
  import BrowserSearch from './BrowserSearch.svelte'
  let { tauri } = $props()
</script>

<div class="language-preferences">
  <p class="settings-intro">Choose the language Muniment and its Browser tab use, and the engine the address bar searches.</p>
  <BrowserLanguage {tauri} />
  <BrowserSearch />
</div>

<style>
  /* One column at the Appearance measure, the intro first, then one section per setting. */
  .language-preferences { display: grid; gap: 28px; max-width: 560px; }
  .language-preferences > .settings-intro { margin-bottom: -6px; }
</style>
