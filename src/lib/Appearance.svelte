<script>
  import ContextSettings from '../activity/ContextSettings.svelte'
  import { onMount } from 'svelte'
  import { DARK_THEMES, DEFAULT_DARK_THEME, DEFAULT_LIGHT_THEME, LIGHT_THEMES, THEME_NAMES, THEME_STORAGE_KEY, THEME_SYSTEM, applyTheme, readStoredTheme, serializeTheme, themeScheme } from './theme-state.js'
  import { SHIPPED_FONTS, SIZE_STEP_MAX, SIZE_STEP_MIN, TYPE_EVENT, bodySize, commitType, filterFonts, readStoredType, stepType, typeSizeShortcut, typeSizeShortcutLabel } from './type-state.js'

  let { tauri } = $props()

  let theme = $state(readStoredTheme())

  // Type: one size step for the body register, and a family for each register
  // picked from the fonts installed on this device. The shipped pair stays the default.
  let type = $state(readStoredType())
  let fonts = $state([])
  let fontsRead = $state(false)
  let query = $state({ human: '', mono: '', heading: '' })
  const registers = [['human', 'Conversation'], ['heading', 'Headers'], ['mono', 'Records']]

  onMount(() => {
    const follow = (event) => { type = event.detail }
    window.addEventListener(TYPE_EVENT, follow)
    void (async () => {
      try {
        const names = await tauri?.invoke('installed_fonts')
        fonts = Array.isArray(names) ? names : []
      } catch (_) {
        fonts = []
      }
      fontsRead = true
    })()
    return () => window.removeEventListener(TYPE_EVENT, follow)
  })

  function step(delta) {
    type = commitType(delta === 0 ? { ...type, step: 0 } : stepType(type, delta))
  }

  function chooseFont(register, family) {
    type = commitType({ ...type, [register]: family })
  }
  const modeOptions = [['System', 'system'], ['Light', 'light'], ['Dark', 'dark']]
  const themeGroups = [['Light', LIGHT_THEMES], ['Dark', DARK_THEMES]]

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
  <h3 id="appearance-heading" class="access-label">Themes</h3>
  <div class="theme-options" role="group" aria-label="Mode">
    {#each modeOptions as option}
      <button type="button" aria-pressed={theme.mode === option[1]} onclick={() => chooseMode(option[1])}>{option[0]}</button>
    {/each}
  </div>
  <details><summary>Browse themes</summary>
  <div class="themes" role="group" aria-labelledby="appearance-heading">
    {#each themeGroups as [label, row]}
      <p class="group-label">{label}</p>
      <div class="theme-grid">
        {#each row as name}
          <button type="button" class="theme-pick" aria-pressed={picked(name)} onclick={() => chooseTheme(name)}>
            <span class="swatch" data-swatch data-theme={name} aria-hidden="true"><span class="swatch-card"><i class="swatch-ink"></i><i class="swatch-muted"></i><i class="swatch-signal"></i></span></span>
            <span>{THEME_NAMES[name]}</span>
          </button>
        {/each}
      </div>
    {/each}
  </div>
  </details>

  <div class="type-section" role="group" aria-labelledby="type-heading">
  <h3 id="type-heading" class="access-label">Type</h3>
  <div class="type-size" role="group" aria-label="Size">
    <button type="button" aria-label="Smaller type" aria-keyshortcuts={typeSizeShortcut('smaller')} disabled={type.step <= SIZE_STEP_MIN} onclick={() => step(-1)}>Smaller <kbd>{typeSizeShortcutLabel('smaller')}</kbd></button>
    <span class="type-readout" role="status">{bodySize(type.step)} px body</span>
    <button type="button" aria-label="Larger type" aria-keyshortcuts={typeSizeShortcut('larger')} disabled={type.step >= SIZE_STEP_MAX} onclick={() => step(1)}>Larger <kbd>{typeSizeShortcutLabel('larger')}</kbd></button>
    <button type="button" class="type-reset" aria-label="Default type size" aria-keyshortcuts={typeSizeShortcut('default')} disabled={type.step === 0} onclick={() => step(0)}>Default <kbd>{typeSizeShortcutLabel('default')}</kbd></button>
  </div>
  {#each registers as [register, label] (register)}
    <div class="type-font" role="group" aria-label="{label} font">
      <details class="font-picker">
      <summary><span>{label}</span><span class="font-current">{type[register] || SHIPPED_FONTS[register]}</span></summary>
      <input type="search" class="font-search" aria-label="Search {label.toLowerCase()} fonts" placeholder="Search installed fonts" bind:value={query[register]}>
      <div class="font-list">
        <button type="button" class="font-pick" aria-pressed={type[register] === null} onclick={() => chooseFont(register, null)}><span class="font-name">{SHIPPED_FONTS[register]}</span><span class="font-note">Default</span></button>
        {#each filterFonts(fonts, query[register]) as name (name)}
          <button type="button" class="font-pick" aria-pressed={type[register] === name} onclick={() => chooseFont(register, name)}><span class="font-name" style:font-family={`'${name}'`}>{name}</span></button>
        {/each}
      </div>
      {#if fontsRead && fonts.length === 0}<p class="font-note">Installed fonts unavailable.</p>{/if}
      </details>
    </div>
  {/each}
  </div>
</section>

<ContextSettings {tauri} />

<style>
  .access-label { margin: 0 0 6px; color: var(--muted); font: var(--text-12) var(--font-mono); text-transform: uppercase; letter-spacing: .04em; }
  .theme-options { display: inline-flex; border: 1px solid var(--border); border-radius: var(--radius-control); }
  .theme-options button { position: relative; font: inherit; font-size: var(--text-13); border: 0; border-radius: 0; background: transparent; color: var(--muted); padding: 5px 12px; cursor: pointer; }
  .theme-options button + button { border-left: 1px solid var(--border); }
  .theme-options button:first-child { border-radius: var(--radius-control) 0 0 var(--radius-control); }
  .theme-options button:last-child { border-radius: 0 var(--radius-control) var(--radius-control) 0; }
  .theme-options button:hover { background: var(--faint); color: var(--ink); }
  .theme-options button[aria-pressed="true"] { background: var(--faint); color: var(--ink); }
  /* The light themes under one label and the dark under another, four to a row, a swatch of each theme's paper and surface beside its name. */
  .themes { margin-top: 10px; max-width: 560px; }
  .group-label { margin: 8px 0 4px; color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .theme-grid { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 6px; }
  .theme-pick { display: flex; align-items: center; gap: 8px; min-width: 0; padding: 6px 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: inherit; font-size: var(--text-13); text-align: left; cursor: pointer; }
  .theme-pick > span:last-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .theme-pick:hover { background: var(--faint); color: var(--ink); }
  .theme-pick[aria-pressed="true"] { background: var(--faint); color: var(--ink); }
  /* The swatch element carries the theme's own tokens: a page of its paper, a surface card on it, an ink line, a muted line and a signal mark. */
  .swatch { flex: none; display: block; box-sizing: border-box; width: 40px; height: 28px; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--paper); }
  .swatch-card { display: grid; align-content: start; gap: 3px; box-sizing: border-box; height: 100%; padding: 3px; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--surface); }
  .swatch i { display: block; height: 2px; }
  .swatch-ink { width: 80%; background: var(--ink); }
  .swatch-muted { width: 50%; background: var(--muted); }
  .swatch-signal { width: 8px; background: var(--signal); }
  /* Type: the size row is three quiet buttons around a mono readout, and each register is one search over the installed families with the shipped family first. */
  .type-section { margin-top: 22px; max-width: 560px; }
  .type-size { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .type-size button { display: inline-flex; align-items: center; gap: 8px; font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 10px; cursor: pointer; }
  .type-size button:hover:not(:disabled) { background: var(--faint); }
  .type-size button:disabled { color: var(--muted); cursor: default; }
  .type-size kbd { font: var(--text-12) var(--font-mono); color: var(--muted); }
  .type-readout { min-width: 96px; text-align: center; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .type-reset { margin-left: 4px; }
  .type-font { margin-top: 10px; }
  .font-picker { border: 1px solid var(--border); border-radius: var(--radius-control); padding: 8px 10px; }
  .font-picker summary { cursor: pointer; font-size: var(--text-13); overflow-wrap: anywhere; }
  .font-current { float: right; max-width: 65%; color: var(--muted); }
  .font-picker[open] summary { margin-bottom: 10px; }
  .font-search { box-sizing: border-box; width: 100%; height: 28px; padding: 0 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; font-size: var(--text-13); }
  .font-search:focus { outline: none; border-color: var(--muted); }
  .font-list { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 4px; margin: 6px 0 0; }
  .font-pick { display: flex; align-items: baseline; justify-content: space-between; gap: 8px; width: 100%; min-width: 0; padding: 6px 8px; border: 1px solid transparent; border-radius: var(--radius-control); background: transparent; color: var(--muted); font: inherit; font-size: var(--text-13); text-align: left; cursor: pointer; }
  .font-pick:hover { background: var(--faint); color: var(--ink); }
  .font-pick[aria-pressed="true"] { background: var(--faint); color: var(--ink); }
  .font-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .font-note { margin: 4px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
</style>
