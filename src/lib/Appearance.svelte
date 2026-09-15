<script>
  import { DARK_THEMES, DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, LIGHT_THEMES, THEME_NAMES, THEME_STORAGE_KEY, THEME_SYSTEM, applyTheme, readStoredTheme, serializeTheme, themeScheme } from './theme-state.js'

  let theme = $state(readStoredTheme())
  const modeOptions = [['System', 'system'], ['Light', 'light'], ['Dark', 'dark']]
  const themeRows = [LIGHT_THEMES, DARK_THEMES]

  function commit(next) {
    theme = next
    applyTheme(document.documentElement, theme)
    try { localStorage.setItem(THEME_STORAGE_KEY, serializeTheme(theme)) } catch (_) {}
  }

  // A mode keeps the last theme picked in it. System shows the two defaults.
  function chooseMode(mode) {
    commit({ ...theme, mode })
  }

  function chooseTheme(name) {
    const scheme = themeScheme(name)
    commit({ ...theme, mode: scheme, [scheme]: name })
  }

  function picked(name) {
    if (theme.mode === THEME_SYSTEM) return name === DEFAULT_LIGHT_THEME || name === DEFAULT_DARK_THEME
    return theme[theme.mode] === name
  }
</script>

<section aria-labelledby="appearance-heading">
  <h3 id="appearance-heading" class="access-label">Appearance</h3>
  <div class="theme-options" role="group" aria-labelledby="appearance-heading">
    {#each modeOptions as option}
      <button type="button" aria-pressed={theme.mode === option[1]} onclick={() => chooseMode(option[1])}>{option[0]}</button>
    {/each}
  </div>
  <div class="theme-grid" role="group" aria-label="Themes">
    {#each themeRows as row}
      {#each row as name}
        <button type="button" class="theme-pick" aria-pressed={picked(name)} onclick={() => chooseTheme(name)}>
          <span class="swatch-ring" aria-hidden="true"><span class="swatch" data-swatch data-theme={name}></span></span>
          <span>{THEME_NAMES[name]}</span>
        </button>
      {/each}
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
  /* Four light themes on one row, four dark on the next: a swatch of the theme's paper and surface beside its name. */
  .theme-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 6px; margin-top: 10px; max-width: 480px; }
  .theme-pick { display: flex; align-items: center; gap: 8px; min-width: 0; padding: 6px 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: inherit; font-size: var(--text-13); text-align: left; cursor: pointer; }
  .theme-pick > span:last-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .theme-pick:hover { background: var(--faint); color: var(--ink); }
  .theme-pick[aria-pressed="true"] { background: var(--faint); color: var(--ink); }
  .swatch-ring { flex: none; display: inline-flex; padding: 1px; border: 1px solid var(--border); border-radius: 50%; }
  /* The swatch element carries the theme's own tokens, so paper fills its left half and surface its right. */
  .swatch { position: relative; display: block; width: 14px; height: 14px; overflow: hidden; border-radius: 50%; background: var(--paper); }
  .swatch::after { content: ''; position: absolute; inset: 0 0 0 50%; background: var(--surface); }
</style>
