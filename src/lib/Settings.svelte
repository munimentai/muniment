<script>
  import ExtendSection from '../extend/ExtendSection.svelte'
  import { featureFlags } from '../feature-flags.js'
  import { panelScroll } from './panel-scroll.js'
  // Settings is one popup over the workspace: a section list on its left, the
  // section on its right, and the workspace darkened and blurred behind it.
  import { onMount, tick } from 'svelte'
  import LucideIcon from './LucideIcon.svelte'
  import Appearance from './Appearance.svelte'
  import MemorySection from './MemorySection.svelte'
  import ModelsSection from './ModelsSection.svelte'
  import AccountSettings from './AccountSettings.svelte'
  import CompaniesSection from './CompaniesSection.svelte'

  let {
    tauri,
    listen = (...args) => window.__TAURI__?.event?.listen(...args),
    section = $bindable('models'),
    onclose,
    homePath = '',
    onchangehome,
    local = false,
    signInDisabled = false,
    onsignin,
    accountStatus = '',
    oninventory,
    inventory = null,
    oncompanieschange = () => {},
    voiceShortcut = '',
    voiceShortcutChanging = false,
    onVoiceShortcutChange = async () => true,
    defaultVoiceShortcut = '',
  } = $props()

  const sections = [['models', 'Models & routing'], ['extend', 'Extend'], ['appearance', 'Preferences'], ['memory', 'Profile & Memory'], ['home', 'Home'], ...(featureFlags.companyRecord ? [['companies', 'Companies']] : []), ...(featureFlags.cloud ? [['account', 'Account']] : [])]
  $effect(() => {
    if ((section === 'companies' && !featureFlags.companyRecord) || (section === 'account' && !featureFlags.cloud)) section = 'models'
  })
  let panel = $state()
  const sectionLabel = $derived(section === 'routing' ? 'Models & routing' : sections.find(([id]) => id === section)?.[1] ?? 'Settings')

  onMount(() => {
    void tick().then(() => panel?.querySelector('[aria-current="true"]')?.focus())
    const onKeydown = (event) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        event.stopPropagation()
        onclose()
        return
      }
      if (event.key !== 'Tab' || !panel) return
      const controls = [...panel.querySelectorAll('button:not(:disabled), input:not(:disabled), textarea:not(:disabled), select:not(:disabled), summary, [href]')]
      if (controls.length === 0) return
      const first = controls[0]
      const last = controls.at(-1)
      if (event.shiftKey && (document.activeElement === first || !panel.contains(document.activeElement))) {
        event.preventDefault()
        last.focus()
      } else if (!event.shiftKey && (document.activeElement === last || !panel.contains(document.activeElement))) {
        event.preventDefault()
        first.focus()
      }
    }
    document.addEventListener('keydown', onKeydown, true)
    return () => {
      document.removeEventListener('keydown', onKeydown, true)
    }
  })

  function scrimClick(event) {
    if (event.target === event.currentTarget) onclose()
  }
</script>

<div class="settings-scrim" data-testid="settings-scrim" onclick={scrimClick}>
  <div data-panel="settings" data-panel-variant="overlay" class="settings-panel" role="dialog" aria-modal="true" aria-labelledby="settings-title" bind:this={panel}>
    <nav class="settings-nav" aria-label="Settings sections">
      <h2 id="settings-title">Settings</h2>
      <ul>
        {#each sections as [id, label]}
          <li><button type="button" class="quiet" aria-current={section === id ? 'true' : undefined} onclick={() => { section = id }}>{label}</button></li>
        {/each}
      </ul>
    </nav>
    <div class="settings-body">
      <header data-panel-header class="settings-head">
        <h3 id="settings-section-title">{sectionLabel}</h3>
        <button type="button" class="quiet close" aria-label="Close settings" onclick={onclose}><LucideIcon name="x" variant="action" size={16} /></button>
      </header>
      <div class="settings-content" use:panelScroll>
        {#if section === 'extend'}
          <ExtendSection {tauri} />
        {:else if section === 'models' || section === 'routing'}
          {#key section}
            <ModelsSection {tauri} {listen} {oninventory} {inventory}  />
          {/key}
        {:else if section === 'appearance'}
          <Appearance {tauri} />
        {:else if section === 'memory'}
          <MemorySection {tauri} />
        {:else if section === 'home'}
          <section class="settings-home" aria-labelledby="settings-home-title">
            <h4 id="settings-home-title" class="settings-label">Home</h4>
            <p class="settings-path">{homePath}</p>
            <p class="support">Muniment keeps memory, agents, projects and sessions here.</p>
            <button type="button" onclick={onchangehome}>Change folder…</button>
          </section>
        {:else if featureFlags.companyRecord && section === 'companies'}
          <CompaniesSection {tauri} onchanged={oncompanieschange} />
        {:else if featureFlags.cloud && section === 'account'}
          <section class="settings-account" aria-labelledby="settings-account-title">
            <h4 id="settings-account-title" class="settings-label">Account</h4>
            {#if local}
              <p class="support">Local mode runs on this device with your own providers. A free account adds the mobile relay.</p>
              <button type="button" disabled={signInDisabled} onclick={onsignin}>Sign in for cloud features</button>
            {/if}
            {#if accountStatus}<p class="support" role="status">{accountStatus}</p>{/if}
            {#if !local}
              <AccountSettings {tauri} {voiceShortcut} {voiceShortcutChanging} {onVoiceShortcutChange} {defaultVoiceShortcut} />
            {/if}
          </section>
        {/if}
      </div>
    </div>
  </div>
</div>

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  .quiet { background: transparent; border-color: transparent; }
  /* The one blur in the app: the workspace under Settings blurs behind the theme's paper, dark in dark mode and light in light mode, and Settings covers most of it. */
  .settings-scrim { position: fixed; inset: 0; z-index: 8; display: grid; place-items: center; padding: 40px; background: var(--overlay-backdrop); -webkit-backdrop-filter: blur(10px); backdrop-filter: blur(10px); }
  .settings-panel { width: min(1080px, 100%); height: min(760px, 100%); display: grid; grid-template-columns: 216px minmax(0, 1fr); overflow: hidden;      }
  .settings-nav { display: flex; flex-direction: column; gap: 4px; min-width: 0; padding: 20px 12px; border-right: 1px solid var(--border); background: var(--paper); }
  .settings-nav h2 { margin: 0 8px 12px; color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .settings-nav ul { display: grid; gap: 2px; margin: 0; padding: 0; list-style: none; }
  .settings-nav button { width: 100%; padding: 7px 10px; text-align: left; font-size: var(--text-13); }
  .settings-nav button[aria-current="true"] { background: var(--faint); }
  .settings-body { display: flex; flex-direction: column; min-width: 0; min-height: 0; }
  .settings-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; padding: var(--panel-control-inset); }
  .settings-head h3 { margin: 0; font-size: var(--text-15); font-weight: 600; }
  .close { min-width: 28px; min-height: 28px; padding: 5px; line-height: 0; }
  .settings-content { flex: 1; min-height: 0; padding: 16px 28px 28px; overflow-y: auto; }
  .settings-home, .settings-account { display: grid; gap: 8px; justify-items: start; }
  .settings-label { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); letter-spacing: .04em; text-transform: uppercase; }
  .settings-path { margin: 0; overflow-wrap: anywhere; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .settings-home button, .settings-account button { min-height: 28px; padding: 4px 10px; }
</style>
