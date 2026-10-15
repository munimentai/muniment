<script>
  import { EXTEND_TABS, MODEL_TABS, pageForTab, settingsPage, settingsSections } from './settings-sections.js'
  import SettingsSection from './SettingsSection.svelte'
  const ExtendSection = () => import('../extend/ExtendSection.svelte')
  import { featureFlags } from '../feature-flags.js'
  import PanelPage from './ui/PanelPage.svelte'
  import PageHead from './ui/PageHead.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Button from './ui/Button.svelte'
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
  const SearchSection = () => import('./SearchSection.svelte')
  const VoiceSection = () => import('./VoiceSection.svelte')
  const RuntimesSection = () => import('../runtimes/RuntimesSection.svelte')
  import { LOCAL_PAGES } from '../runtimes/runtimes.js'

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
    connectProvider = null,
    // The selected paired runtime, or null while this computer's runtime is selected.
    remoteRuntime = null,
    runtimeSwitchDisabled = false,
  } = $props()

  const sections = $derived(settingsSections({ remote: !!remoteRuntime }))
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
      // The workspace panel beside Settings keeps Escape for its terminal and browser.
      if (event.target?.closest?.('[data-panel="workspace"]')) return
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
  <PanelPage>
    {#if remoteRuntime && LOCAL_PAGES.includes(section)}
      <p class="support" role="note">These settings belong to this computer. {remoteRuntime.name} keeps its own.</p>
    {/if}
    {#if section in EXTEND_TABS}
      <SettingsSection load={ExtendSection} properties={{tauri, oncreate: oncreateextension, sidebarTab: EXTEND_TABS[section]}} />
    {:else if section in MODEL_TABS}
      <SettingsSection load={ModelsSection} properties={{tauri, listen, oninventory, inventory, sidebarTab: MODEL_TABS[section], ontab: showModelTab, connect: section === 'providers' ? connectProvider : null}} />
    {:else if section === 'appearance'}
      <SettingsSection load={Appearance} properties={{tauri}} />
    {:else if section === 'context'}
      <SettingsSection load={ContextSettings} properties={{tauri}} />
    {:else if section === 'voice'}
      <SettingsSection load={VoiceSection} properties={{tauri, voiceShortcut, voiceShortcutChanging, onVoiceShortcutChange, defaultVoiceShortcut}} />
    {:else if section === 'search'}
      <SettingsSection load={SearchSection} properties={{tauri}} />
    {:else if section === 'language'}
      <SettingsSection load={LanguageSection} properties={{tauri}} />
    {:else if section === 'profile' || section === 'memory'}
      <SettingsSection load={MemorySection} properties={{tauri, part: section}} />
    {:else if section === 'home'}
      <PageHead intro="Muniment keeps your work and its extensions in two places on this device." />
      <Group heading="Folders">
        <Row title="Workspace folder" description="Holds memory, agents, projects and session files.">
          {#snippet detail()}<p class="settings-path">{homePath}</p>{/snippet}
          <Button onclick={onchangehome}>Change workspace folder…</Button>
        </Row>
        <Row title="Managed extensions" description="Installed skills and plugins, kept apart from your workspace.">
          {#snippet detail()}<p class="settings-path">~/.muniment</p>{/snippet}
          <Button onclick={() => tauri.invoke('extend_command', {action:'open_folder',data:{}}).catch(() => storageError = 'The extension folder could not open.')}>Open extension folder</Button>
        </Row>
        {#if storageError}<p role="alert">{storageError}</p>{/if}
      </Group>
    {:else if section === 'runtimes'}
      <SettingsSection load={RuntimesSection} properties={{tauri, listen, switchDisabled: runtimeSwitchDisabled}} />
    {:else if featureFlags.companyRecord && section === 'companies'}
      <SettingsSection load={CompaniesSection} properties={{tauri, onchanged:oncompanieschange, onrecord}} />
    {:else if featureFlags.cloud && section === 'account' && !remoteRuntime}
      {#if local}
        <PageHead intro="Local mode runs on this device with your own providers. A free account adds the mobile relay." />
        <Group heading="Account">
          <Row title="Sign in" description="Pair your phone and use your devices on one account.">
            <Button disabled={signInDisabled} onclick={onsignin}>Sign in for cloud features</Button>
          </Row>
          {#if accountStatus}<p role="status">{accountStatus}</p>{/if}
        </Group>
      {:else}
        <PageHead intro="Your account links this desktop to your phone and devices, and holds your plan." />
        {#if accountStatus}<p class="support" role="status">{accountStatus}</p>{/if}
        <SettingsSection load={AccountSettings} properties={{tauri, subject, onsignout}} />
      {/if}
    {/if}
  </PanelPage>
</div>

<style>
  /* Settings takes the thread and rail columns under the title band. The sidebar beside it lists the sections. */
  .settings-layout { grid-row: 2; grid-column: 2 / -1; position: relative; z-index: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
  .settings-path { margin: 2px 0 0; overflow-wrap: anywhere; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
</style>
