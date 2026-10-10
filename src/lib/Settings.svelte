<script>
  import { EXTEND_TABS, MODEL_TABS, pageForTab, settingsPage, settingsSections } from './settings-sections.js'
  import SettingsSection from './SettingsSection.svelte'
  const ExtendSection = () => import('../extend/ExtendSection.svelte')
  import { featureFlags } from '../feature-flags.js'
  import { panelScroll } from './panel-scroll.js'
  // Settings takes over the workspace: the sidebar lists its pages under group
  // headings, and this panel shows the picked page across the thread and rail.
  import { onMount } from 'svelte'
  const Appearance = () => import('./Appearance.svelte')
  const MemorySection = () => import('./MemorySection.svelte')
  const ModelsSection = () => import('./ModelsSection.svelte')
  const AccountSettings = () => import('./AccountSettings.svelte')
  const CompaniesSection = () => import('./CompaniesSection.svelte')
  const ContextSettings = () => import('../activity/ContextSettings.svelte')
  const LanguageSection = () => import('./LanguageSection.svelte')

  let {
    tauri,
    listen = (...args) => window.__TAURI__?.event?.listen(...args),
    section = $bindable('providers'),
    onclose,
    homePath = '',
    oncreateextension,
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
    subject = '',
    onsignout = null,
    onrecord = null,
  } = $props()

  const sections = settingsSections()
  $effect(() => {
    if (settingsPage(section) !== section) section = settingsPage(section)
    else if (!sections.some(([id]) => id === section)) section = 'providers'
  })
  let storageError = $state('')
  let panel = $state()
  const sectionLabel = $derived(sections.find(([id]) => id === settingsPage(section))?.[1] ?? 'Settings')
  // A Models tab that moves itself, such as Connect account from Decision models, moves the sidebar with it.
  const showModelTab = (tab) => { section = pageForTab(MODEL_TABS, tab) ?? section }

  onMount(() => {
    const onKeydown = (event) => {
      // Native child dialogs own focus and Escape until they close.
      if (event.target?.closest?.('dialog[open]')) return
      const activeDialog = event.target?.closest?.('[role="dialog"]')
      if (activeDialog && activeDialog !== panel) return
      if (event.key === 'Escape') {
        event.preventDefault()
        event.stopPropagation()
        onclose()
        return
      }
    }
    document.addEventListener('keydown', onKeydown, true)
    return () => {
      document.removeEventListener('keydown', onKeydown, true)
    }
  })

</script>

<div data-panel="settings" class="settings-layout" role="dialog" aria-label="Settings" bind:this={panel}>
  <!-- The sidebar already names the page, so its heading is for assistive tech only. -->
  <h3 id="settings-section-title" class="visually-hidden">{sectionLabel}</h3>
  <div class="settings-content" use:panelScroll>
    {#if section in EXTEND_TABS}
      <SettingsSection load={ExtendSection} properties={{tauri, oncreate: oncreateextension, sidebarTab: EXTEND_TABS[section]}} />
    {:else if section in MODEL_TABS}
      <SettingsSection load={ModelsSection} properties={{tauri, listen, oninventory, inventory, sidebarTab: MODEL_TABS[section], ontab: showModelTab}} />
    {:else if section === 'appearance'}
      <SettingsSection load={Appearance} properties={{tauri}} />
    {:else if section === 'context'}
      <SettingsSection load={ContextSettings} properties={{tauri}} />
    {:else if section === 'language'}
      <SettingsSection load={LanguageSection} properties={{tauri}} />
    {:else if section === 'profile' || section === 'memory'}
      <SettingsSection load={MemorySection} properties={{tauri, part: section}} />
    {:else if section === 'home'}
      <section class="settings-home" aria-labelledby="settings-home-title">
        <p class="settings-intro">Muniment keeps your work and its extensions in two places on this device.</p>
        <h4 id="settings-home-title" class="settings-heading">Workspace folder</h4>
        <p class="settings-path">{homePath}</p>
        <p class="support">Your workspace holds memory, agents, projects and session files.</p>
        <button type="button" onclick={onchangehome}>Change workspace folder…</button>
        <h4 class="settings-heading">Managed extensions</h4>
        <p class="settings-path">~/.muniment</p>
        <p class="support">Installed skills and plugins live in managed storage, separate from your workspace.</p>
        <button type="button" onclick={() => tauri.invoke('extend_command', {action:'open_folder',data:{}}).catch(() => storageError = 'The extension folder could not open.')}>Open extension folder</button>
        {#if storageError}<p role="alert">{storageError}</p>{/if}
      </section>
    {:else if featureFlags.companyRecord && section === 'companies'}
      <SettingsSection load={CompaniesSection} properties={{tauri, onchanged:oncompanieschange, onrecord}} />
    {:else if featureFlags.cloud && section === 'account'}
      <section class="settings-account" aria-label="Account">
        {#if local}
          <p class="settings-intro">Local mode runs on this device with your own providers. A free account adds the mobile relay.</p>
          <button type="button" disabled={signInDisabled} onclick={onsignin}>Sign in for cloud features</button>
        {/if}
        {#if accountStatus}<p class="support" role="status">{accountStatus}</p>{/if}
        {#if !local}
          <p class="settings-intro">Your account links this desktop to your phone and devices, and holds your plan.</p>
          <SettingsSection load={AccountSettings} properties={{tauri, voiceShortcut, voiceShortcutChanging, onVoiceShortcutChange, defaultVoiceShortcut, subject, onsignout}} />
        {/if}
      </section>
    {/if}
  </div>
</div>

<style>
  button { font: inherit; font-size: var(--text-13); color: var(--ink); background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-control); padding: 5px 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--faint); }
  button:disabled { color: var(--muted); cursor: default; }
  /* Settings takes the thread and rail columns under the title band. The sidebar beside it lists the sections. */
  .settings-layout { grid-row: 2; grid-column: 2 / -1; position: relative; z-index: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
  .settings-content { flex: 1; min-height: 0; padding: 32px 36px 36px; overflow-y: auto; }
  .settings-home { display: grid; gap: 8px; justify-items: start; }
  .settings-account { display: grid; gap: 22px; }
  .settings-account > button { justify-self: start; }
  .settings-home .settings-heading { margin-top: 14px; }
  .settings-path { margin: 0; overflow-wrap: anywhere; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
  .settings-home button, .settings-account button { min-height: 28px; padding: 4px 10px; }
</style>
