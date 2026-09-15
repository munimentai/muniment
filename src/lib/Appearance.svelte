<script>
  import { THEME_STORAGE_KEY, parseTheme, serializeTheme } from './theme-state.js'

  let theme = $state(readTheme())
  const themeOptions = [['System', 'system'], ['Light', 'light'], ['Dark', 'dark']]

  function readTheme() {
    try {
      return parseTheme(localStorage.getItem(THEME_STORAGE_KEY))
    } catch (_) {
      return parseTheme(null)
    }
  }

  function chooseTheme(choice) {
    theme = parseTheme(choice)
    if (theme === 'system') delete document.documentElement.dataset.theme
    else document.documentElement.dataset.theme = theme
    try { localStorage.setItem(THEME_STORAGE_KEY, serializeTheme(theme)) } catch (_) {}
  }
</script>

<section aria-labelledby="appearance-heading">
  <h3 id="appearance-heading" class="access-label">Appearance</h3>
  <div class="theme-options" role="group" aria-labelledby="appearance-heading">
    {#each themeOptions as option}
      <button type="button" aria-pressed={theme === option[1]} onclick={() => chooseTheme(option[1])}>{option[0]}</button>
    {/each}
  </div>
</section>

<style>
  .access-label { margin: 0 0 6px; color: var(--muted); font: var(--text-12) var(--font-mono); text-transform: uppercase; letter-spacing: .04em; }
  .theme-options { display: inline-flex; border: 1px solid var(--border); border-radius: var(--radius-control); }
  .theme-options button { position: relative; font: inherit; font-size: var(--text-13); border: 0; border-radius: 0; background: transparent; color: var(--muted); padding: 5px 12px; cursor: pointer; }
  .theme-options button + button { border-left: 1px solid var(--border); }
  .theme-options button:first-child { border-radius: var(--radius-control) 0 0 var(--radius-control); }
  .theme-options button:last-child { border-radius: 0 var(--radius-control) var(--radius-control) 0; }
  .theme-options button:hover { background: var(--faint); color: var(--ink); }
  .theme-options button[aria-pressed="true"] { background: var(--faint); color: var(--ink); }
</style>
