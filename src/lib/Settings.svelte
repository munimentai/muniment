<script>
  import { settingsSections, settingsTarget } from './settings-sections.js'
  import SettingsSection from './SettingsSection.svelte'
  import SettingsPart from './SettingsPart.svelte'
  const ExtendSection = () => import('../extend/ExtendSection.svelte')
  import { featureFlags } from '../feature-flags.js'
  import PanelPage from './ui/PanelPage.svelte'
  import PageHead from './ui/PageHead.svelte'
  import Group from './ui/Group.svelte'
  import Row from './ui/Row.svelte'
  import Button from './ui/Button.svelte'
  import FailureNote from './FailureNote.svelte'
  // Settings takes over the workspace: the sidebar lists its seven pages, and
  // this panel shows the picked page across the thread and rail. A page that
  // holds several parts, such as Data, shows each under its own head.
  import { onMount, setContext } from 'svelte'
  const Appearance = () => import('./Appearance.svelte')
  const MemorySection = () => import('./MemorySection.svelte')
  const ModelsSection = () => import('./ModelsSection.svelte')
  const AccountSettings = () => import('./AccountSettings.svelte')
  const CloudVault = () => import('../runtimes/CloudVault.svelte')
  const CompaniesSection = () => import('./CompaniesSection.svelte')
  const ContextSettings = () => import('../activity/ContextSettings.svelte')
  const PermissionSettings = () => import('./PermissionSettings.svelte')
  const LanguageSection = () => import('./LanguageSection.svelte')
  const NotificationSettings = () => import('../notifications/NotificationSettings.svelte')
  const VoiceSection = () => import('./VoiceSection.svelte')
  const RuntimesSection = () => import('../runtimes/RuntimesSection.svelte')
  const ThreadRetention = () => import('./ThreadRetention.svelte')
  import { ALL_MACHINES, machineLocal, scopeNote, settingsMachine } from '../runtimes/runtimes.js'
  import MachineScope from '../runtimes/MachineScope.svelte'
  import { agentRequest } from '../agents/agent-runtime.js'

  let {
    tauri,
    listen = (...args) => window.__TAURI__?.event?.listen(...args),
    section = $bindable('models'),
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
    // Every machine, from `runtime_list`, and the switch to another one.
    runtimeList = null,
    onselectruntime = () => {},
    // Called after a permission setting saves, so the composer reads the modes again.
    onpermissions = () => {},
  } = $props()

  const sections = settingsSections()
  // `section` names a page, or a part of one, or an older page id that now
  // sits inside a page. The sidebar marks the page and the part scrolls into view.
  const target = $derived(settingsTarget(section))
  const page = $derived(sections.some(([id]) => id === target[0]) ? target[0] : 'models')
  const part = $derived(page === target[0] ? target[1] : null)
  let panel = $state()
  const machine = $derived(settingsMachine(runtimeList, page))
  // Settings opens on All machines. Picking one machine selects it, and
  // picking All machines again keeps the selected machine for the composer.
  let scope = $state(ALL_MACHINES)
  const allMachines = $derived(!!machine && scope === ALL_MACHINES)
  const machineLabel = $derived(remoteRuntime?.name ?? 'this computer')
  function pickScope(choice) {
    scope = choice === ALL_MACHINES ? ALL_MACHINES : choice.id
    if (choice !== ALL_MACHINES && choice.id !== machine?.selected.id) onselectruntime(choice)
  }
  const sectionLabel = $derived(sections.find(([id]) => id === page)?.[1] ?? 'Settings')
  // Each page's PageHead shows the sidebar label as its title.
  setContext('page-title', () => sectionLabel)
  // Where each part keeps its values comes from core's `settings.scope` on
  // this computer. A part that stays on each machine says so while Settings
  // changes all machines, and a part this computer keeps says so while
  // another machine is selected.
  let settingsScope = $state(null)
  const localNote = (id) => allMachines ? scopeNote(settingsScope, id, { all: true, machine: machineLabel }) : remoteRuntime ? scopeNote(settingsScope, id, { machine: remoteRuntime.name }) : ''

  onMount(() => {
    Promise.resolve().then(() => agentRequest(tauri, null, 'settings.scope')).then((answer) => { settingsScope = answer ?? null }, () => {})
    const onKeydown = (event) => {
      // Native child dialogs own focus and Escape until they close.
      if (event.target?.closest?.('dialog[open]')) return
      const activeDialog = event.target?.closest?.('[role="dialog"]')
      if (activeDialog && activeDialog !== panel) return
      // The workspace panel beside Settings keeps Escape for its terminal and browser.
      if (event.target?.closest?.('[data-panel="workspace"]')) return
      // The platform's find shortcut moves to the Settings search.
      if ((event.metaKey || event.ctrlKey) && !event.altKey && event.key.toLowerCase() === 'f') {
        const search = document.getElementById('settings-search')
        if (search) {
          event.preventDefault()
          search.focus()
          search.select()
          return
        }
      }
      // Escape clears a search before it closes Settings.
      if (event.key === 'Escape' && event.target?.id === 'settings-search' && event.target.value) return
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
  <PanelPage>
    {#if machine}<MachineScope scope={machine} all={allMachines} note={allMachines ? localNote(page) : ''} disabled={runtimeSwitchDisabled} onselect={pickScope} />{/if}
    <!-- A page reads its machines when it opens, so switching them opens it again. -->
    {#key `${allMachines ? ALL_MACHINES : remoteRuntime?.id ?? 'local'}:${page}`}
    {#if page === 'models'}
      <SettingsSection load={ModelsSection} properties={{tauri, listen, oninventory, inventory, focus: part, connect: connectProvider}} />
    {:else if page === 'tools'}
      <SettingsSection load={ExtendSection} properties={{tauri, oncreate: oncreateextension, focus: part, localNote: localNote('mcp')}} />
    {:else if page === 'machines'}
      <SettingsSection load={RuntimesSection} properties={{tauri, listen, switchDisabled: runtimeSwitchDisabled, focus: part}} />
    {:else if page === 'permissions'}
      <SettingsSection load={PermissionSettings} properties={{tauri, runtime: remoteRuntime?.id ?? null, machineName: allMachines ? 'every machine' : machineLabel, all: allMachines, local: machineLocal(settingsScope, 'permissions'), onsaved: onpermissions}} />
    {:else if page === 'account'}
      <PageHead intro="Your profile, and the MuniCloud account that holds your plan." />
      <SettingsPart id="profile" title="Profile" focus={part === 'profile'}>
        <SettingsSection load={MemorySection} properties={{tauri, part: 'profile'}} />
      </SettingsPart>
      {#if featureFlags.cloud && !remoteRuntime}
        <SettingsPart id="cloud" title="MuniCloud" focus={part === 'cloud' || part === 'vault'}>
          {#if local}
            <PageHead intro="Local mode runs on this device with your own providers. A free account adds the mobile relay." />
            <Group label="Account">
              <Row title="Sign in" description="Pair your phone and devices on one account.">
                <Button variant="primary" disabled={signInDisabled} onclick={onsignin}>Sign in for cloud features</Button>
              </Row>
              {#if accountStatus}<FailureNote failure={accountStatus} />{/if}
            </Group>
          {:else}
            <PageHead intro="Your account holds your plan. Its phone and devices are on Machines." />
            {#if accountStatus}<FailureNote failure={accountStatus} />{/if}
            {#snippet vault()}<SettingsSection load={CloudVault} properties={{tauri, listen}} />{/snippet}
            <SettingsSection load={AccountSettings} properties={{tauri, subject, onsignout, children: vault}} />
          {/if}
        </SettingsPart>
      {/if}
    {:else if page === 'data'}
      <PageHead intro="What Muniment keeps: memories, conversation context and files." />
      <SettingsPart id="memory" title="Memory" focus={part === 'memory'}>
        <SettingsSection load={MemorySection} properties={{tauri, part: 'memory'}} />
      </SettingsPart>
      <SettingsPart id="context" title="Context" note={localNote('context')} focus={part === 'context'}>
        <SettingsSection load={ContextSettings} properties={{tauri}} />
      </SettingsPart>
      <SettingsPart id="retention" title="Retention" note={localNote('retention')} focus={part === 'retention'}>
        <SettingsSection load={ThreadRetention} properties={{tauri}} />
      </SettingsPart>
      <SettingsPart id="storage" title="Storage" note={localNote('storage')} focus={part === 'storage'}>
        <PageHead intro="Your workspace folder on this device." />
        <Group label="Folders">
          <Row title="Workspace folder" description="Memory, agents, projects and sessions.">
            {#snippet detail()}<p class="settings-path">{homePath}</p>{/snippet}
            <Button onclick={onchangehome}>Change workspace folder…</Button>
          </Row>
        </Group>
      </SettingsPart>
      {#if featureFlags.companyRecord}
        <SettingsPart id="companies" title="Companies" note={localNote('companies')} focus={part === 'companies'}>
          <SettingsSection load={CompaniesSection} properties={{tauri, onchanged:oncompanieschange, onrecord}} />
        </SettingsPart>
      {/if}
    {:else if page === 'general'}
      <PageHead intro="How Muniment looks, listens, reads and notifies, on every machine." />
      <SettingsPart id="appearance" title="Appearance" focus={part === 'appearance'}>
        <SettingsSection load={Appearance} properties={{tauri}} />
      </SettingsPart>
      <SettingsPart id="language" title="Language" focus={part === 'language'}>
        <SettingsSection load={LanguageSection} properties={{tauri}} />
      </SettingsPart>
      <SettingsPart id="voice" title="Voice" focus={part === 'voice'}>
        <SettingsSection load={VoiceSection} properties={{tauri, voiceShortcut, voiceShortcutChanging, onVoiceShortcutChange, defaultVoiceShortcut}} />
      </SettingsPart>
      <SettingsPart id="notifications" title="Notifications" focus={part === 'notifications'}>
        <SettingsSection load={NotificationSettings} properties={{tauri}} />
      </SettingsPart>
      <SettingsPart id="about" title="About" focus={part === 'about'}>
        <PageHead />
        <Group label="Version">
          <Row title="Muniment" meta={runtimeList?.appVersion ? `v${String(runtimeList.appVersion).replace(/^v/, '')}` : ''} description="Updates install from the title bar, and Machines updates the others." />
        </Group>
      </SettingsPart>
    {/if}
    {/key}
  </PanelPage>
</div>

<style>
  /* Settings takes the thread and rail columns under the title band. The sidebar beside it lists the sections. */
  .settings-layout { grid-row: 2; grid-column: 2 / -1; position: relative; z-index: 1; display: flex; flex-direction: column; min-width: 0; min-height: 0; overflow: hidden; }
  .settings-path { margin: 2px 0 0; overflow-wrap: anywhere; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .support { margin: 0; color: var(--muted); font-size: var(--text-13); }
</style>
