<script>
  import { featureFlags } from './feature-flags.js'
  import { floatingMenu } from './lib/floating-menu.js'
  import { panelScroll } from './lib/panel-scroll.js'
  import UserQuestion from './lib/UserQuestion.svelte'
  import { THREAD_ORGANIZATION_KEY, readThreadOrganization, organizeThreads } from './lib/thread-organization.js'
  import ComposerReferences from './composer/ComposerReferences.svelte'
  import FileMentions from './composer/FileMentions.svelte'
  import { mentionQuery, insertMention, composerParts } from './composer/composer-references.js'
  import { responseParts } from './activity/action-feedback.js'
  import ActionFeedback from './activity/ActionFeedback.svelte'
  import { onMount, tick, untrack } from 'svelte'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { getCurrentWindow, UserAttentionType } from '@tauri-apps/api/window'
  import { open } from '@tauri-apps/plugin-dialog'
  import { register, unregister } from '@tauri-apps/plugin-global-shortcut'
  import { openUrl } from '@tauri-apps/plugin-opener'

  import AccessPanel from './lib/AccessPanel.svelte'
  const loadSettings = () => import('./lib/Settings.svelte')
  import ThreadFilter from './lib/ThreadFilter.svelte'
  import ProjectCatalog from './lib/ProjectCatalog.svelte'
  import CatalogActions from './lib/CatalogActions.svelte'
  import ArtifactCatalog from './lib/ArtifactCatalog.svelte'
  import {creationOptions, creationDefaults, creationPrompt} from './lib/creation-prompts.js'
  import AgentManager from './lib/AgentManager.svelte'
  import WorkspacePanel from './lib/WorkspacePanel.svelte'
  import WorkspaceMenu from './lib/WorkspaceMenu.svelte'
  import AgentAvatar from './lib/AgentAvatar.svelte'
  import AgentProfile from './lib/AgentProfile.svelte'
  import ChatComposer from './composer/ChatComposer.svelte'
  import ModelPicker from './lib/ModelPicker.svelte'
  import { COMPOSER_PANEL_EVENT, openComposerPanel } from './lib/composer-panels.js'
  import Capacity from './lib/Capacity.svelte'
  import { currentModel, currentModelAvailable, modelChipLabel } from './lib/provider-catalog.js'
  import { classifierProvider } from './lib/classifier-catalog.js'
  import ProviderLogo from './lib/ProviderLogo.svelte'
  import LucideIcon from './lib/LucideIcon.svelte'
  import RowControl from './lib/RowControl.svelte'
  import AssistantMarkdown from './lib/AssistantMarkdown.svelte'
  import FilePanel from './files/FilePanel.svelte'
  import FileChanges from './files/FileChanges.svelte'
  import { changedFiles, fileName } from './files/file-changes.js'
  import CodeDiff from './lib/CodeDiff.svelte'
  import ConfirmDialog from './lib/ConfirmDialog.svelte'
  import Onboarding from './lib/Onboarding.svelte'
  import { commitType, readStoredType, stepType, typeSizeShortcutStep } from './lib/type-state.js'
  import { ARTIFACT_RAIL_MAX_WIDTH, ARTIFACT_RAIL_MIN_WIDTH, artifactRailShortcut, createRailController, defaultArtifactRailWidth, isArtifactRailShortcut, isRecordPanelShortcut, railBounds, recordPanelShortcut, shortcutDisplayLabel } from './lib/artifact-rail-state.js'
  import RecordPanel from './record/RecordPanel.svelte'
  import { bootState, errorState, registrationRetryState, statusState, waitingState } from './lib/auth-state.js'
  import { createBackgroundServiceNotice } from './lib/background-service-notice.js'
  import GraphMark from './lib/GraphMark.svelte'
  import { codeDiffPermissionAnswer, composerAction, formatByteSize, messageLocalTime, permissionGateAction, permissionGateCommitHint, receiptLabel, receiptRows, receiptSummary, receiptUsageColumns, runAnnouncement, runFailureMessage } from './lib/chat-state.js'
  import ComposerExtensions from './extend/ComposerExtensions.svelte'
  import { createChatController } from './lib/chat-controller.js'
  import { listenForLauncher } from './lib/launcher-bridge.js'
  import { composerHeight } from './lib/composer-size.js'
  import { createDictationController } from './lib/dictation-controller.js'
  import { ariaKeyShortcut, holdToTalkShortcut, isDictationActive } from './lib/dictation-state.js'
  import { createEntitlementToast } from './lib/entitlement-toast.js'
  import { createChatTranscriptController } from './lib/chat-transcript-controller.js'
  import { copyAnnouncement, copyConfirmed, copyFailure, copyLabel } from './lib/message-actions.js'
  import { onboardingLoadingState, onboardingSettingsState } from './lib/onboarding-state.js'
  import { firstRunError } from './lib/onboarding-diagnostics.js'
  import { relativeTime } from './lib/relative-time.js'
  import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, SIDEBAR_STORAGE_KEY, createSidebarResizeController, isNewThreadShortcut, isSettingsShortcut, isSidebarShortcut, newThreadShortcut, settingsShortcut, serializeSidebarCollapsed, sidebarShortcut, storedSidebarCollapsed, storedSidebarWidth, threadRowShortcut, threadRowShortcutPosition } from './lib/sidebar-state.js'
  import { formatBytes, installStateWords } from './lib/speech-install.js'
  import RunMark from './lib/RunMark.svelte'
  import ExecutionTime from './lib/ExecutionTime.svelte'
  import ProjectRow from './lib/ProjectRow.svelte'
  import { threadTitle, creationTitle } from './lib/thread-title.js'
  import { createVoiceGesture } from './lib/voice-gesture.js'
  import { createVoiceShortcutManager } from './lib/voice-shortcut.js'
  import { createWindowTitle } from './lib/window-title.js'

  const version = __APP_VERSION__

  function boundedAttachClaim(value) {
    const claim = typeof value === 'string' ? [...value] : []
    return claim.length > 0
      && claim.length <= 80
      && claim.join('').trim()
      && !claim.some((character) => /[\p{Cc}\p{Cf}]/u.test(character))
      ? value
      : 'unknown'
  }

  function boundedAttachWorkspace(value) {
    const workspace = typeof value === 'string'
      ? [...value].filter((character) => !/[\p{Cc}\p{Cf}]/u.test(character)).join('')
      : ''
    return [...workspace].length > 0 && [...workspace].length <= 80 && workspace.trim()
      ? workspace
      : 'unknown'
  }

  const tauri = window.__TAURI__?.core
  let auth = $state(bootState)
  let localEntryError = $state('')
  let accountStatus = $state('')
  // The connected providers and their models, as Settings → Models and the picker read them.
  let inventory = $state(null)
  let hasSubscriptions = $state(false)
  $effect(() => {
    if (!inventory) { hasSubscriptions = false; return }
    let disposed = false
    void tauri.invoke('model_router_settings').then(settings => {
      if (!disposed) {
        hasSubscriptions = (settings?.accounts ?? []).some(account => account.source === 'account')
        if (!hasSubscriptions) capacityOpen = false
      }
    }).catch(() => { if (!disposed) { hasSubscriptions = false; capacityOpen = false } })
    return () => { disposed = true }
  })
  let inventoryError = $state('')
  let inventoryRequestVersion = 0
  let pickerOpen = $state(false)
  let capacityOpen = $state(false)
  $effect(() => {
    const panel = event => {
      pickerOpen = event.detail === 'model'
      capacityOpen = event.detail === 'capacity'
      if (event.detail !== 'speech') speechInstallDismissed = true
      if (event.detail !== 'mentions') mention = null
    }
    const outside = event => {
      if (composerBox?.contains(event.target) || event.target.closest?.('[data-panel="question"]')) return
      openComposerPanel(null)
    }
    window.addEventListener(COMPOSER_PANEL_EVENT, panel)
    document.addEventListener('pointerdown', outside, true)
    return () => { window.removeEventListener(COMPOSER_PANEL_EVENT, panel); document.removeEventListener('pointerdown', outside, true) }
  })
  let modelChoicePending = $state(false)
  let retryReview = $state(null)
  let modelChip = $state()

  const composerHint = $derived(
    active?.phase === 'resuming' ? 'Reopening the existing secure session…'
      : active ? ''
        : auth.name === 'local' ? ''
          : 'Routing is automatic. Every reply carries its receipt.')

  const modelSourceLabel = $derived(modelChipLabel(inventory))
  const chipModel = $derived(currentModel(inventory))

  let settingsOpen = $state(false)
  let agentsExpanded = $state(false)
  let artifactsExpanded = $state(false)
  let agentsOpen = $state(false)
  let artifactsOpen = $state(false)
  let projectsOpen = $state(false)
  let projectsExpanded = $state(true)
  let creations = $state([])
  let artifactItems = $state([])
  let requestedArtifact = $state(null)
  const currentCreation = $derived(pendingCreation || creations.find(item => item.threadId === currentThreadId))
  const artifactChats = $derived([
    ...creations.filter(item => item.kind === 'artifact').map(item => ({...item,name:creationTitle(item, threadSummaries, artifactItems.find(a=>a.id===item.resultId)?.name)})),
    ...artifactItems.filter(item => !creations.some(p=>p.threadId === item.threadId || p.resultId === item.id) && artifactItems.find(a=>a.threadId===item.threadId)?.id===item.id),
  ])
  const CATALOG_ARCHIVE_KEY = 'muniment.catalog-archive'
  function readCatalogArchive() {
    try { return JSON.parse(localStorage.getItem(CATALOG_ARCHIVE_KEY) || '{}') } catch (_) { return {} }
  }
  let catalogArchive = $state(readCatalogArchive())
  function catalogKey(kind, item) { return `${kind === 'creation' ? item.kind : kind}:${item.id || item.threadId}` }
  function catalogArchived(kind, item) { return catalogArchive[catalogKey(kind,item)] === true }
  async function catalogAction(kind, item, action, name) {
    if (active || threadSwitching) throw new Error('Finish the current reply before changing this item.')
    const plans = kind === 'agent' ? creations.filter(plan=>plan.resultId===item.id) : creations.filter(plan=>plan.threadId===item.threadId)
    if (action === 'archive' || action === 'restore') {
      if (kind === 'agent' && action === 'archive' && item.schedule?.enabled) {
        await tauri.invoke('agent_save',{agent:{...item,schedule:{...item.schedule,enabled:false}}})
        await refreshAgents()
      }
      const next = {...catalogArchive,[catalogKey(kind,item)]:action==='archive'}
      localStorage.setItem(CATALOG_ARCHIVE_KEY,JSON.stringify(next))
      catalogArchive = next
      return
    }
    if (action === 'rename') {
      if (kind === 'agent') {
        await tauri.invoke('agent_save',{agent:{...item,name}})
        await refreshAgents()
      } else {
        const id = kind === 'artifact' && (item.resultId || item.id)
        if (id) await tauri.invoke('artifact_edit',{id,name})
        else await tauri.invoke('creation_save',{creation:{...item,goal:name}})
        await refreshCreations()
      }
      return
    }
    if (action === 'delete') {
      const threadIds = kind === 'agent' ? Object.keys(agentListing.state.threads).filter(id=>agentListing.state.threads[id]===item.id) : [item.threadId].filter(Boolean)
      const next = {...threadOrganization}
      for (const id of [...threadIds,...plans.map(plan=>plan.threadId)]) next[id] = {...next[id],archived:true}
      localStorage.setItem(THREAD_ORGANIZATION_KEY,JSON.stringify(next))
      threadOrganization = next
      if (kind === 'agent') {
        await tauri.invoke('agent_delete',{id:item.id})
        if (selectedAgent===item.id) { selectedAgent=null;agentProfileOpen=false }
      } else if (kind === 'artifact' && (item.resultId || item.id)) {
        await tauri.invoke('artifact_edit',{id:item.resultId || item.id,name:null})
        if (requestedArtifact === (item.resultId || item.id)) { requestedArtifact=null;browserPanel=null }
      }
      for (const plan of plans) await tauri.invoke('creation_delete',{threadId:plan.threadId})
      await Promise.all([refreshCreations(),refreshAgents()])
    }
  }
  let creationsRevision = 0
  async function refreshCreations() {
    const revision = ++creationsRevision
    const values = await Promise.allSettled([tauri.invoke('creation_list'),tauri.invoke('artifact_list')])
    if (revision !== creationsRevision) return
    if (values[0].status === 'fulfilled' && Array.isArray(values[0].value)) creations = values[0].value
    if (values[1].status === 'fulfilled' && Array.isArray(values[1].value)) artifactItems = values[1].value
  }
  async function startCreation(kind, chosen = creationDefaults(kind)) {
    if (active || threadSwitching) return
    if (!await newSidebarThread(selectedProject) || !currentThreadId) return
    pendingCreation = { threadId: currentThreadId, kind, goal: chosen.goal, output: chosen.output, resultId: null }
    draft = creationPrompt(kind, pendingCreation)
    await tick()
    composer?.focus()
  }
  async function useCreationSuggestion(chosen) {
    if (pendingCreation) {
      pendingCreation = { ...pendingCreation, goal: chosen.goal, output: chosen.output }
      draft = creationPrompt(pendingCreation.kind, pendingCreation)
    }
    await tick()
    composer?.focus()
  }
  function cancelCreation() {
    pendingCreation = null
    draft = ''
    selectedFiles = []
    turnExtensions = { enabled: [], blocked: [], automatic: false }
    composer?.focus()
  }
  async function openCreation(item) {
    if (active || threadSwitching) return
    if (!item.threadId) {
      if (!await newSidebarThread(selectedProject) || !currentThreadId) return
      try {
        const plan=await tauri.invoke('creation_save',{creation:{threadId:currentThreadId,kind:'artifact',goal:`Maintain ${item.name}`,output:'The saved artifact and its revisions',resultId:item.id}})
        creationsRevision += 1
        creations=[...creations,plan]
      } catch(e) {submitError=String(e);return}
    } else if (!await chatController.openThread(item.threadId)) return
    projectsOpen=false; agentsOpen=false; artifactsOpen=false; agentProfileOpen=false
    const artifactId=item.resultId || item.id
    if (artifactId && item.kind !== 'agent') {requestedArtifact=artifactId;showBrowser('artifacts')}
  }
  async function showArtifacts() {
    await refreshCreations()
    artifactsOpen=true;projectsOpen=false; agentsOpen=false;agentProfileOpen=false;closeRail()
  }
  let browserPanel = $state(null)
  let workspacePanelWidth = $state(500)
  let workspacePanelMaximum = $state(1200)
  let workspacePanelPointer = $state()

  let toolsMenuOpen = $state(false)
  let requestedNavigation = $state(null)
  function openChatLink(url) { requestedNavigation = {url}; showBrowser('browser') }
  let requestedWorkspaceFile = $state(null)
  function showBrowser(mode) { if (mode === 'artifacts') { artifactsOpen = false; projectsOpen = false; agentsOpen = false }; browserPanel = mode; agentProfileOpen = false; closeRail(); workspaceResize.fit() }
  let agentsRequest = $state(0)
  let agentPanelId = $state(null)
  let agentCreateNew = $state(false)
  async function showAgents(id = null, create = false) {
    await Promise.all([refreshAgents(),refreshCreations()])
    if (create) return startCreation('agent')
    projectsOpen=false; artifactsOpen=false; agentsOpen=true; agentProfileOpen=false; agentsRequest+=1;closeRail()
  }
  let agentProfileOpen = $state(false)
  let agentPanelWidth = $state(320)
  let agentPanelMaximum = $state(560)
  let agentPanelPointer = $state()
  let lastProfileAgent = null
  function revealAgentProfile() { browserPanel = null; projectsOpen = false; agentsOpen = false; closeRail(); agentProfileOpen = true; agentResize.fit() }
  $effect(() => {
    const id = profileAgent?.id
    if (id && id !== lastProfileAgent && !browserPanel) untrack(() => revealAgentProfile())
    lastProfileAgent = id
  })
  let agentOpening = $state(false)
  let selectedAgent = $state(null)
  const profileAgent = $derived(agentListing.agents.find(agent => agent.id === selectedAgent))
  let agentListing = $state({ agents: [], state: { threads: {}, runs: {} } })
  async function refreshAgents() {
    try { const value = await tauri.invoke('agent_list'); if (value?.agents) { agentListing = value; selectedAgent = value.state.threads[currentThreadId] ?? null } }
    catch (_) { /* The manager presents errors and retry state. */ }
  }
  async function openAgent(agent) {
    if (active || threadSwitching || agentOpening) return
    agentOpening = true
    try {
      const mapping = agentListing.state.threads
      const threadId = agentListing.state.primaryThreads?.[agent.id] ?? (mapping[currentThreadId] === agent.id ? currentThreadId
        : threadSummaries.find(thread => mapping[thread.threadId] === agent.id && !threadOrganization[thread.threadId]?.archived)?.threadId
          ?? Object.keys(mapping).find(id => mapping[id] === agent.id && !threadOrganization[id]?.archived))
      if (threadId) {
        if (threadId !== currentThreadId && !await chatController.openThread(threadId, true)) return
      } else await startAgentThread(agent)
      selectedAgent = agent.id
      selectedProject = agent.projectId || null
      projectsOpen = false; agentsOpen = false; artifactsOpen = false
      if (!browserPanel) revealAgentProfile()
    } finally { agentOpening = false }
  }
  async function startAgentThread(agent) {
    selectedAgent = agent.id
    selectedProject = agent.projectId || null
    if (!await chatController.newThread()) { selectedAgent = agentListing.state.threads[currentThreadId] ?? null; throw new Error('The agent thread could not be started.') }
    threadSearch = ''
    archivedThreads = false
    filterThreads()
    await refreshAgents()
  }
  let settingsSection = $state('models')
  let settingsButton = $state()
  // The control that opened Settings takes focus back when it closes.
  let settingsOpener = null

  function openSettings(section = 'models', opener = settingsButton) {
    settingsSection = section
    settingsOpener = opener ?? null
    pickerOpen = false
    settingsOpen = true
  }

  async function createExtensionDraft(kind) {
    if (active || threadSwitching) return
    if (!await newSidebarThread(selectedProject)) return
    closeSettings()
    draft = `Help me create a ${kind}. Ask about its purpose, then save it in managed extension storage.`
    await tick()
    composer?.focus()
  }

  function closeSettings() {
    settingsOpen = false
    const opener = settingsOpener ?? settingsButton ?? composer
    void tick().then(() => opener?.focus?.())
  }

  function toggleSettings() {
    if (settingsOpen) closeSettings()
    else openSettings(settingsSection)
  }

  // The platform's settings shortcut toggles the popup.
  function settingsShortcutPressed() {
    toggleSettings()
  }

  // The super key with =, - or 0 moves the body type size one step, the same
  // step Preferences offers, and 0 returns the default.
  function typeSizeShortcutPressed(step) {
    const current = readStoredType()
    commitType(step === 0 ? { ...current, step: 0 } : stepType(current, step))
  }

  function openHomeSettings() {
    settingsOpen = false
    onboarding = onboardingSettingsState(onboarding)
  }

  function closePicker() {
    pickerOpen = false
    void tick().then(() => modelChip?.focus())
  }

  function togglePicker() {
    if (pickerOpen) closePicker()
    else openComposerPanel('model')
  }

  async function chooseModel(provider, model) {
    if (modelChoicePending) return
    modelChoicePending = true
    const before = inventory
    if (inventory) inventory = { ...inventory, default_provider: provider, default_model: model }
    closePicker()
    try {
      await tauri.invoke('local_mode_set_default_model', { provider, model })
      if (retryReview) retryReview = { ...retryReview, chooseModel: retryReview.chooseModel && retryReview.previousProvider === provider && retryReview.previousModel === model }
    } catch (_) {
      if (inventory && before) inventory = { ...inventory, default_provider: before.default_provider, default_model: before.default_model }
      accountStatus = 'Muniment could not save the model choice. Try again.'
    } finally { modelChoicePending = false }
  }

  function prepareRetry(run, chooseModel = false) {
    if (active || (draft.trim() && draft !== run.prompt) || selectedFiles.length || !run.prompt?.trim()) return
    draft = run.prompt
    retryReview = { threadId: currentThreadId, chooseModel, previousProvider: chipModel?.provider, previousModel: chipModel?.model }
    if (chooseModel) openComposerPanel('model')
    void tick().then(() => composer?.focus())
  }

  $effect(() => {
    if (retryReview && retryReview.threadId !== currentThreadId) retryReview = null
  })

  function openModelSettings() {
    pickerOpen = false
    openSettings('models', modelChip)
  }
  const taskDrafts = new Map()
  let pendingCreation = $state(null)
  let turnExtensions = $state({ enabled: [], blocked: [], automatic: false })
  function selectTask(next) {
    if (next === currentThreadId) return
    taskDrafts.set(currentThreadId, { text: draft, files: selectedFiles, extensions: turnExtensions, creation: pendingCreation })
    const saved = taskDrafts.get(next) || (currentThreadId === null && next ? taskDrafts.get(null) : null)
    draft = saved?.text || ''
    selectedFiles = saved?.files || []
    turnExtensions = saved?.extensions || { enabled: [], blocked: [], automatic: false }
    pendingCreation = saved?.creation || null
    currentThreadId = next
  }
  let draft = $state('')
  let selectedFiles = $state([])
  let submitError = $state('')
  let messages = $state([])
  let active = $state(null)
  // The transcript is not a live region. Only the run the user is waiting on is
  // announced, and only when its phase changes. Restored history announces the
  // run the desktop rejoins and nothing else.
  let announcedRun = $state(null)
  let announcement = $derived(runAnnouncement(announcedRun))
  let cancelError = $state('')
  let queueError = $state('')
  let threadListError = $state('')
  let threadListErrorAction = $state(null)
  let historyError = $state('')
  let historyErrorAction = $state(null)
  let threadSummaries = $state([])
  let projectCatalog = $state({ projects: {}, threads: {} })
  let selectedProject = $state(null)
  let catalogProject = $state(null)
  const fileContext = $derived(agentsOpen ? {catalog:'agents'} : artifactsOpen ? {catalog:'artifacts'} : projectsOpen ? (catalogProject ? {projectId:catalogProject} : {catalog:'projects'}) : selectedAgent ? {agentId:selectedAgent} : currentCreation?.kind === 'artifact' && currentCreation.resultId ? {artifactId:currentCreation.resultId} : {threadId:currentThreadId, projectId:projectCatalog.threads[currentThreadId] || null, threadTitle:currentThreadTitle})
  let expandedProjects = $state(new Set())
  let projectForm = $state(null)
  let projectName = $state('')
  let projectBusy = $state(false)
  let projectError = $state('')
  const projectRows = $derived(Object.entries(projectCatalog.projects).sort((a, b) => a[1].localeCompare(b[1])))
  const regularThreads = $derived(threadSummaries.filter(thread => !agentListing.state.threads[thread.threadId] && !creations.some(item=>item.threadId===thread.threadId) && !artifactItems.some(item=>item.threadId===thread.threadId)))
  let threadOrganization = $state(readThreadOrganization())
  let threadSearch = $state('')
  let archivedThreads = $state(false)
  let includeArchived = $state(false)
  let threadSort = $state('recent')
  let threadIndexFailed = $state(false)
  let rowRename = $state(null)
  let rowRenameInput = $state()
  let rowRenamePending = $state(false)
  const threadGroups = $derived(organizeThreads((threadSort === 'title' ? [...regularThreads].sort((a,b) => (a.title || '').localeCompare(b.title || '')) : regularThreads).filter(thread => includeArchived || archivedThreads || !threadOrganization[thread.threadId]?.archived), threadOrganization, threadSearch, archivedThreads, includeArchived))
  const unpinnedThreads = $derived(threadGroups.filter(group => group.name !== 'Pinned').flatMap(group => group.threads))
  const sidebarSections = $derived([...projectRows.filter(([id]) => expandedProjects.has(id)).map(([id]) => id), null]
    .map(projectId => ({ projectId, threads: unpinnedThreads.filter(thread => (projectCatalog.threads[thread.threadId] || null) === projectId) })))
  const pinnedThreads = $derived(threadGroups.find(group => group.name === 'Pinned')?.threads || [])
  const visibleThreads = $derived([...pinnedThreads, ...sidebarSections.flatMap(section => section.threads)])
  const showFreshThread = $derived(freshThread && !selectedAgent && !currentCreation && !archivedThreads && !threadSearch.trim() && (!selectedProject || expandedProjects.has(selectedProject)))
  const shortcutThreads = $derived([...pinnedThreads, ...sidebarSections.flatMap(section => [
    ...(showFreshThread && section.projectId === selectedProject ? [{ threadId: null }] : []), ...section.threads,
  ])])
  const freshThreadPosition = $derived(shortcutThreads.findIndex(thread => thread.threadId === null))
  let moreThreads = $state(false)
  let loadingOlderThreads = $state(false)
  let currentThreadId = $state(null)
  let currentThreadTitle = $derived(profileAgent?.name || (currentCreation ? creationTitle(currentCreation, threadSummaries, artifactItems.find(item=>item.id===currentCreation.resultId)?.name) : threadSummaries.find(({ threadId }) => threadId === currentThreadId)?.title || threadTitle(messages)))
  let freshThread = $state(false)
  let threadSwitching = $state(false)
  let editingThreadTitle = $state(false)
  let threadTitleDraft = $state('')
  let threadTitleInput = $state()
  let threadTitleButton = $state()
  let threadTitleBeforeEdit = ''
  let deletingThreadId = $state(null)
  let deletePending = $state(false)
  let deleteFromTitle = $state(false)
  let titleActionsButton = $state()
  // The rows a confirm covers, and the rows a Shift or Command click gathered.
  let deletingThreadIds = $state([])
  let selectedThreadIds = $state(new Set())
  let selectionAnchor = null
  let permissionAnswer = $state(null)
  let permissionValues = $state({})
  let thread = $state()
  let pinned = $state(true)
  let hasContentBelow = $state(false)
  let expandedReceipts = $state(new Set())
  let parallelTools = $state(new Map())
  let draggingFiles = $state(false)
  let dictation = $state({ state: 'idle' })
  let dictationError = $state('')
  let dictationCommandPending = $state(false)
  let dictationRequested = false
  let dictationDraftSnapshot = $state('')
  let dictationTranscript = $state('')
  let dictationFinishing = $state(false)
  let speechInstallFacts = $state(null)
  let speechInstallStatus = $state(null)
  let speechInstallError = $state('')
  let speechInstallPending = $state(false)
  let speechInstallTimer
  let speechInstallEpoch = 0
  let speechInstallDismissed = $state(false)
  let speechInstallNotice = $state('')
  let globalVoiceRegistered = false
  let globalVoiceError = $state(false)
  let globalVoiceShortcutValue = $state(holdToTalkShortcut())
  let globalVoiceChanging = $state(true)
  let composer = $state()
  // The composer box's height, so the thread's bottom padding and the fade above the composer follow it.
  let composerBox = $state()
  let composerBoxHeight = $state(120)
  let composerRow = $state()
  let composerInputDraft
  let wasInWorkspace = false
  let onboarding = $state(onboardingLoadingState)
  // The one rail column: null, 'artifacts' or 'record'. Opening one closes the other.
  let railOccupant = $state(null)
  let artifactRailOpen = $derived(railOccupant === 'artifacts')
  let viewedFile = $state(null)
  let filePanelOpen = $derived(railOccupant === 'files')
  const changed = $derived(changedFiles(messages))
  $effect(() => {
    const files = changed.filter(file => !file.deleted && /\.html?$/i.test(file.path))
    const threadId = currentThreadId
    if (active || !threadId || !files.length) return
    void Promise.allSettled(files.map(file => tauri.invoke('artifact_from_file', {threadId, path:file.path})))
  })
  function openFile(file) { requestedWorkspaceFile = { ...file, name: file.name || fileName(file.path) }; showBrowser(`file:${file.path}`) }
  let recordPanelOpen = $derived(railOccupant === 'record')
  let artifactRailWidth = $state(defaultArtifactRailWidth(window.innerWidth))
  let artifactRailMaximum = $state(ARTIFACT_RAIL_MAX_WIDTH)
  let artifactRailPointer = $state()
  let recordMaximized = $state(false)
  // The record panel reloads what it shows when this changes: after a run
  // ends, because the agent may have committed, and when the window regains
  // focus, because a harness outside the app may have.
  let recordRefresh = $state(0)
  let recordRefreshActive = null
  $effect(() => {
    const ended = recordRefreshActive !== null && active === null
    recordRefreshActive = active
    if (ended && recordPanelOpen) recordRefresh = untrack(() => recordRefresh) + 1
  })
  let workspace = $state()
  let entitlementToastVisible = $state(false)
  let pairingRequests = $state([])
  let desktopClientStatus = $state(null)
  let desktopClientStatusVersion = 0
  let runtimeNotice = $state(null)
  let backgroundServiceNoticeVisible = $derived(runtimeNotice?.visible === true)
  let authRequestVersion = 0
  let signInCancelControl = $state()
  let signInPending = null
  let signInFromLocal = $state(false)
  $effect(() => { if (auth.name === 'signing-in' && signInFromLocal) signInCancelControl?.focus() })

  async function cancelSignIn() {
    if (auth.name !== 'signing-in' || localEntryPending) return
    authRequestVersion += 1
    localEntryPending = true
    try {
      await tauri.invoke('local_mode_enter')
      markerStartupLocalMode = true
      await signInPending
      auth = { name: 'local', subject: null }
      await refreshInventory()
      await chatController.loadHistory()
    } catch { localEntryError = 'Local mode could not start. Try again.' }
    finally { localEntryPending = false }
  }
  let localEntryPending = $state(false)
  let markerStartupLocalMode = null
  let markerStartupReady = Promise.resolve(false)
  let startupReady = Promise.resolve()
  let retryStartup = false
  const macOS = navigator.platform.startsWith('Mac')
  const artifactShortcut = artifactRailShortcut()
  const recordShortcut = recordPanelShortcut()
  let destroyed = false
  let sidebarWidth = $state(storedSidebarWidth())
  let sidebarMaximum = $state(SIDEBAR_MAX_WIDTH)
  let sidebarPointer = $state()
  const minimumThreadWidth = 320
  let sidebarCollapsed = $state(storedSidebarCollapsed())
  let threadMenu = $state(null)
  const sidebarKeyShortcut = sidebarShortcut()
  const newThreadKeyShortcut = newThreadShortcut()
  const settingsKeyShortcut = settingsShortcut()
  const modifierLabel = shortcutDisplayLabel(sidebarKeyShortcut).slice(0, -1)
  // The newest copy attempt in the thread, or null once its confirmation lapses.
  let copy = $state(null)
  const windowTitle = createWindowTitle(window.__TAURI__?.window?.getCurrentWindow?.())

  const transcriptController = createChatTranscriptController({
    tick,
    clipboard: navigator.clipboard,
    readThread: () => thread,
    readPinned: () => pinned,
    readDestroyed: () => destroyed,
    onPinned: (next) => { pinned = next },
    onContentBelow: (next) => { hasContentBelow = next },
    onCopy: (next) => { copy = next },
    readExpandedReceipts: () => expandedReceipts,
    onExpandedReceipts: (next) => { expandedReceipts = next },
    readParallelTools: () => parallelTools,
    onParallelTools: (next) => { parallelTools = next },
  })
  const { scrollToLatest, followNewContent, handleScroll: handleThreadScroll, copyResponse, toggleReceipt } = transcriptController

  const railController = createRailController({
    readOccupant: () => railOccupant,
    onOccupant: (next) => { railOccupant = next; if (next) agentProfileOpen = false },
    readWidth: () => artifactRailWidth,
    onWidth: (next) => { artifactRailWidth = next },
    readMaximum: () => artifactRailMaximum,
    onMaximum: (next) => { artifactRailMaximum = next },
    readPointer: () => artifactRailPointer,
    onPointer: (next) => { artifactRailPointer = next },
    readAvailableWidth: availableRailWidth,
    readRightEdge: () => (workspace?.getBoundingClientRect().right || window.innerWidth) - workspaceFrameWidth(),
    readViewportWidth: () => workspace?.clientWidth || window.innerWidth,
    readMaximized: () => recordMaximized,
    onMaximized: (next) => { recordMaximized = next },
  })
  const workspaceResize = createRailController({
    readOccupant: () => 'workspace', onOccupant: () => {},
    readWidth: () => workspacePanelWidth, onWidth: value => workspacePanelWidth = value,
    readMaximum: () => workspacePanelMaximum, onMaximum: value => workspacePanelMaximum = value,
    readPointer: () => workspacePanelPointer, onPointer: value => workspacePanelPointer = value,
    readAvailableWidth: () => Math.min(1200, Math.max(340, (workspace?.clientWidth || window.innerWidth) - ((workspace?.clientWidth || window.innerWidth) <= 950 || sidebarCollapsed ? 0 : sidebarWidth) - 360)),
    readRightEdge: () => (workspace?.getBoundingClientRect().right || window.innerWidth) - workspaceFrameWidth(),
    readViewportWidth: () => workspace?.clientWidth || window.innerWidth,
  })
  const agentResize = createRailController({
    readOccupant: () => 'agent', onOccupant: () => {},
    readWidth: () => agentPanelWidth, onWidth: value => agentPanelWidth = value,
    readMaximum: () => agentPanelMaximum, onMaximum: value => agentPanelMaximum = value,
    readPointer: () => agentPanelPointer, onPointer: value => agentPanelPointer = value,
    readAvailableWidth: () => Math.min(560, Math.max(240, (workspace?.clientWidth || window.innerWidth) - sidebarWidth - 360)),
    readRightEdge: () => (workspace?.getBoundingClientRect().right || window.innerWidth) - workspaceFrameWidth(),
    readViewportWidth: () => workspace?.clientWidth || window.innerWidth,
  })
  const { fit: fitArtifactRail, pointerDown: artifactRailPointerDown, pointerMove: artifactRailPointerMove, pointerEnd: artifactRailPointerEnd, keydown: artifactRailKeydown } = railController
  const toggleArtifactRail = () => { if (artifactsOpen) artifactsOpen = false; else void showArtifacts() }
  const toggleRecordPanel = () => { if (!featureFlags.companyRecord) return; artifactsOpen = false; browserPanel = null; projectsOpen = false; agentsOpen = false; agentProfileOpen = false; railController.toggle('record') }
  const closeRail = () => railController.close()
  // Ask puts the open view's SQL into the composer as a fenced block, so the reply starts from what the person sees.
  function askAboutView(sql) {
    if (!sql) return
    const fence = '```sql\n' + sql + '\n```'
    draft = draft.trim() ? `${draft.trimEnd()}\n\n${fence}\n` : `${fence}\n`
    void tick().then(() => composer?.focus())
  }
  const toggleRecordMaximized = () => railController.toggleMaximized()

  const sidebarResizeController = createSidebarResizeController({
    readWidth: () => sidebarWidth,
    readMaximum: () => sidebarMaximum,
    readPointer: () => sidebarPointer,
    readAvailableWidth: availableSidebarWidth,
    readLeftEdge: () => (workspace?.getBoundingClientRect().left || 0) + workspaceFrameWidth(),
    onWidth: (next) => { sidebarWidth = next },
    onMaximum: (next) => { sidebarMaximum = next },
    onPointer: (next) => { sidebarPointer = next },
  })
  const { fit: fitSidebar, pointerDown: sidebarPointerDown, pointerMove: sidebarPointerMove, pointerEnd: sidebarPointerEnd, keydown: sidebarKeydown } = sidebarResizeController

  let composerExtensions = $state()
  let extensionCommandNames = $state([])
  const chatController = createChatController({
    invoke: (command, args) => {
      const submit = () => {
        const preparation = command === 'chat_submit' ? composerExtensions?.prepare(args.prompt) : null
        const send = () => tauri.invoke(command, ...(args === undefined ? [] : [args]))
        return preparation ? preparation.then(send).then(result => { composerExtensions?.submitted(); return result }) : send()
      }
      if (command !== 'chat_submit' || !pendingCreation) return submit()
      return tauri.invoke('creation_save', { creation: pendingCreation }).then(plan => {
        creationsRevision += 1
        creations = [...creations.filter(item => item.threadId !== plan.threadId), plan]
        pendingCreation = null
        return submit()
      })
    },
    listen: (...args) => window.__TAURI__?.event?.listen(...args),
    readMessages: () => messages,
    readActive: () => active,
    readAnnounced: () => announcedRun,
    readDraft: () => draft,
    readFiles: () => selectedFiles,
    readThreadId: () => currentThreadId,
    readThreadSummaries: () => threadSummaries,
    blocked: () => dictationBusy() || runtimeUpgradePending() || modelChoicePending || !!retryReview?.chooseModel,
    onMessages: (next) => { messages = next },
    onActive: (next) => { active = next },
    onAnnounce: (next) => { announcedRun = next },
    onDraft: (next) => { draft = next },
    onFiles: (next) => { selectedFiles = next },
    onSubmitError: (next) => { submitError = next },
    onCancelError: (next) => { cancelError = next },
    onQueueError: (next) => { queueError = next },
    onThreadListError: (next, action) => { threadListError = next; threadListErrorAction = action },
    onHistoryError: (next, action) => { historyError = next; historyErrorAction = action },
    onThreadSummaries: (next) => { threadSummaries = next },
    onMoreThreads: (next) => { moreThreads = next },
    readProject: () => selectedProject,
    readAgent: () => selectedAgent,
    onThreadSelected: (next) => { selectTask(next); selectedAgent = agentListing.state.threads[next] ?? null; if (!selectedAgent) agentProfileOpen = false; void refreshProjects(next); void refreshAgents(); void refreshCreations() },
    onThreadSwitch: (next) => { threadSwitching = next },
    onFreshThread: (next) => { freshThread = next },
    onHistoryStart: () => { expandedReceipts = new Set(); parallelTools = new Map() },
    onHistoryLoaded: () => { pinned = true },
    onFollow: followNewContent,
    onFocus: () => tick().then(() => composer?.focus()),
    onSend: () => { retryReview = null },
    onSignInLink: (link) => { if (auth.name === 'signing-in') auth = { ...auth, link } },
  })

  async function loadOlderThreads() {
    loadingOlderThreads = true
    const firstThreadId = await chatController.loadOlderThreads()
    loadingOlderThreads = false
    if (!firstThreadId) return
    await tick()
    document.querySelector(`[data-thread-id="${CSS.escape(firstThreadId)}"]`)?.focus()
  }

  $effect(() => {
    const needsIndex = includeArchived || projectsOpen || expandedProjects.size || threadSearch.trim() || archivedThreads || Object.entries(threadOrganization)
      .some(([id, state]) => state.pinned && !state.archived && !threadSummaries.some((thread) => thread.threadId === id))
    if (needsIndex && moreThreads && !loadingOlderThreads && !threadIndexFailed && !projectBusy) {
      void untrack(indexOlderThreads)
    }
  })

  async function indexOlderThreads() {
    loadingOlderThreads = true
    const first = await chatController.loadOlderThreads()
    threadIndexFailed = !first && moreThreads
    loadingOlderThreads = false
  }

  async function refreshProjects(threadId) {
    try {
      const catalog = await tauri.invoke('project_list')
      if (!catalog?.projects || !catalog?.threads) return
      projectCatalog = catalog
      if (threadId && threadId === currentThreadId) selectedProject = catalog.threads[threadId] || null
      projectError = ''
    } catch (_) { projectError = 'Projects could not be loaded.' }
  }

  async function showProjects() {
    projectsOpen = true; agentsOpen = false; artifactsOpen = false; agentProfileOpen = false; closeRail()
    await refreshProjects()
  }
  async function createCatalogProject(name) {
    projectForm = 'new'; projectName = name
    await saveProject()
    return !projectError
  }
  function selectProject(projectId) {
    const next = new Set(expandedProjects)
    if (next.has(projectId)) next.delete(projectId)
    else next.add(projectId)
    expandedProjects = next
  }

  async function newProjectThread(projectId) {
    if (active || threadSwitching || projectBusy) return
    expandedProjects = new Set([...expandedProjects, projectId])
    await newSidebarThread(projectId)
  }

  async function saveProject() {
    if (projectBusy || !projectName.trim()) return
    projectBusy = true
    try {
      const creating = projectForm === 'new'
      const id = creating ? await tauri.invoke('project_create', { name: projectName.trim() }) : projectForm
      if (projectForm !== 'new') await tauri.invoke('project_rename', { projectId: id, name: projectName.trim() })
      await refreshProjects()
      projectForm = null
      projectName = ''
      projectError = ''
      projectBusy = false
      if (creating) { projectsExpanded = true; await selectProject(id) }
    } catch (error) { projectError = String(error); projectBusy = false }
  }

  async function openProjectFolder(projectId) {
    try { await tauri.invoke('project_open', { projectId }); projectError = '' }
    catch (_) { projectError = 'The project folder could not be opened.' }
  }

  async function newSidebarThread(projectId = null) {
    artifactsOpen = false
    if (active || threadSwitching || projectBusy) return
    selectedProject = projectId
    projectsOpen = false
    agentsOpen = false
    agentProfileOpen = false
    selectedAgent = null
    if (!await chatController.newThread()) return
    threadSearch = ''
    archivedThreads = false
    filterThreads()
    return true
  }

  function filterThreads() {
    threadIndexFailed = false
    clearThreadSelection()
    closeThreadMenu()
  }

  function setThreadOrganization(threadId, field) {
    const fromTitle = threadMenu?.fromTitle
    const previous = threadOrganization[threadId] || {}
    const next = { ...threadOrganization, [threadId]: { ...previous, [field]: !previous[field] } }
    try {
      localStorage.setItem(THREAD_ORGANIZATION_KEY, JSON.stringify(next))
      threadOrganization = next
      clearThreadSelection()
      closeThreadMenu()
      if (fromTitle) void tick().then(() => titleActionsButton?.focus())
      else if (field === 'archived') void tick().then(() => document.querySelector('.archive-toggle')?.focus())
      else focusThreadRow(threadId)
    } catch (_) {
      historyError = 'The thread choice could not be saved. Try again.'
    }
  }

  function renameSidebarThread(summary) {
    closeThreadMenu()
    rowRename = { threadId: summary.threadId, title: summary.title || 'New thread', previous: summary.title || 'New thread' }
    void tick().then(() => rowRenameInput?.select())
  }

  async function saveSidebarName() {
    if (!rowRename || rowRenamePending || !rowRename.title.trim()) return
    const edit = { ...rowRename }
    rowRenamePending = true
    const saved = edit.title.trim() === edit.previous || await chatController.renameThread(edit.title, edit.previous, edit.threadId)
    rowRenamePending = false
    if (saved) { rowRename = null; focusThreadRow(edit.threadId) }
  }

  function sidebarNameKeydown(event) {
    if (event.key === 'Enter') { event.preventDefault(); void saveSidebarName() }
    if (event.key === 'Escape' && !rowRenamePending) {
      event.preventDefault()
      const id = rowRename.threadId
      rowRename = null
      focusThreadRow(id)
    }
  }

  const entitlementToast = createEntitlementToast({
    listen: (...args) => window.__TAURI__?.event?.listen(...args),
    onVisible: (visible) => { entitlementToastVisible = visible },
  })

  const backgroundServiceNotice = createBackgroundServiceNotice({
    invoke: (...args) => tauri.invoke(...args),
    listen: (...args) => window.__TAURI__?.event?.listen(...args),
    onChange: (notice) => { runtimeNotice = notice },
  })

  function applyDesktopClientStatus(status) {
    // The runtime drops a chat-event subscriber whose queue fills, and the
    // desktop resubscribes after a retry. No window saw the events inside that
    // gap, so the shell reads the open thread and the newest list page again
    // on the recovery. The first status compares against no earlier status, so
    // it re-reads nothing, unless a history read already failed before the
    // connection came up: that read repeats once the runtime connects.
    const chatEventsRecovered = desktopClientStatus?.chat_events_connected === false
      && status?.chat_events_connected === true
    const requestsRecovered = status?.connected === true
      && (desktopClientStatus?.connected === false || (!desktopClientStatus && chatController.historyReadFailed()))
    desktopClientStatus = status
    if (chatEventsRecovered || (requestsRecovered && workspaceMode())) {
      void startupReady.then(() => chatController.recoverChatEvents())
    }
  }

  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed
    try { localStorage.setItem(SIDEBAR_STORAGE_KEY, serializeSidebarCollapsed(sidebarCollapsed)) } catch (_) {}
    fitArtifactRail()
  }

  function editThreadTitle(title) {
    if (editingThreadTitle) return
    threadTitleBeforeEdit = title
    threadTitleDraft = title
    editingThreadTitle = true
    void tick().then(() => threadTitleInput?.select())
  }

  function threadTitleKeydown(event) {
    if (event.key === 'Escape') {
      event.preventDefault()
      threadTitleDraft = threadTitleBeforeEdit
      editingThreadTitle = false
      void tick().then(() => threadTitleButton?.focus())
    } else if (event.key === 'Enter') {
      event.preventDefault()
      commitThreadTitle()
    }
  }

  function threadTitleButtonKeydown(event) {
    if (event.key !== 'Enter' && event.key !== ' ') return
    event.preventDefault()
    editThreadTitle(currentThreadTitle)
  }

  function commitThreadTitle() {
    if (!editingThreadTitle) return
    editingThreadTitle = false
    void chatController.renameThread(threadTitleDraft, threadTitleBeforeEdit)
    void tick().then(() => threadTitleButton?.focus())
  }

  function limitThreadTitle(event) {
    const limited = Array.from(event.currentTarget.value).slice(0, 80).join('')
    threadTitleDraft = limited
    event.currentTarget.value = limited
  }

  // A Shift click takes the range from the last picked row, a Command or
  // Control click adds or drops one row, and a plain click opens the thread.
  function threadRowClick(event, threadId) {
    if (event.shiftKey || event.metaKey || event.ctrlKey) {
      event.preventDefault()
      selectThread(threadId, event.shiftKey)
      return
    }
    projectsOpen = false
    agentsOpen = false
    artifactsOpen = false
    clearThreadSelection()
    void chatController.openThread(threadId)
  }

  // The open thread's row is not a button, so a modifier click alone selects it.
  function currentRowClick(event, threadId) {
    if (!(event.shiftKey || event.metaKey || event.ctrlKey)) return
    event.preventDefault()
    event.currentTarget.focus()
    selectThread(threadId, event.shiftKey)
  }

  function selectThread(threadId, range) {
    const next = new Set(selectedThreadIds)
    const ids = shortcutThreads.map(item => item.threadId).filter(Boolean)
    if (range && selectionAnchor && ids.includes(selectionAnchor)) {
      const [from, to] = [ids.indexOf(selectionAnchor), ids.indexOf(threadId)].sort((a, b) => a - b)
      for (const id of ids.slice(from, to + 1)) next.add(id)
    } else {
      if (next.has(threadId)) next.delete(threadId)
      else next.add(threadId)
      selectionAnchor = threadId
    }
    selectedThreadIds = next
  }

  function clearThreadSelection() {
    if (selectedThreadIds.size) selectedThreadIds = new Set()
    selectionAnchor = null
  }

  // The rows one delete covers: the selection when the row is part of it, else the row alone.
  function deletionTargets(threadId) {
    return selectedThreadIds.has(threadId) && selectedThreadIds.size > 1 ? [...selectedThreadIds] : [threadId]
  }

  // The control's name, and the confirm group's spoken question.
  function deleteLabel(threadId, rowTitle, form = 'name') {
    const count = deletionTargets(threadId).length
    if (count > 1) return form === 'name' ? `Delete ${count} threads…` : `Delete ${count} threads?`
    return form === 'name' ? `Delete ${rowTitle}` : `Delete ${rowTitle}?`
  }

  function askToDeleteThread(threadId) {
    deleteFromTitle = !!threadMenu?.fromTitle
    closeThreadMenu()
    deletingThreadIds = deletionTargets(threadId)
    deletingThreadId = threadId
  }

  function threadRowKeydown(event, threadId) {
    if (event.key === 'Delete' || event.key === 'Backspace') {
      if (active || threadSwitching) return
      event.preventDefault()
      askToDeleteThread(threadId)
    } else if (event.key === 'Escape' && selectedThreadIds.size) {
      event.preventDefault()
      clearThreadSelection()
    }
  }

  function focusThreadRow(threadId) {
    void tick().then(() => {
      Array.from(document.querySelectorAll('[data-thread-id]'))
        .find((row) => row.dataset.threadId === threadId)?.focus()
    })
  }

  function cancelDeleteThread() {
    const threadId = deletingThreadId
    deletingThreadId = null
    deletingThreadIds = []
    if (deleteFromTitle) void tick().then(() => titleActionsButton?.focus())
    else focusThreadRow(threadId)
    deleteFromTitle = false
  }

  // The row's actions live in a menu on right-click, Control-click or the
  // keyboard's context menu key, placed at the pointer like the native one.
  function openThreadMenu(event, threadId, fromTitle = false) {
    if (deletingThreadId === threadId) return
    event.preventDefault()
    if (selectedThreadIds.size && !selectedThreadIds.has(threadId)) clearThreadSelection()
    const row = event.currentTarget.getBoundingClientRect()
    const fromKeyboard = !event.clientX && !event.clientY
    threadMenu = { threadId, fromTitle, x: fromTitle || fromKeyboard ? row.left : event.clientX, y: fromTitle || fromKeyboard ? row.bottom : event.clientY }
    document.addEventListener('pointerdown', closeThreadMenuOutside, true)
  }

  function closeThreadMenuOutside(event) {
    if (event.target.closest?.('.thread-menu')) return
    closeThreadMenu()
  }

  function closeThreadMenu() {
    document.removeEventListener('pointerdown', closeThreadMenuOutside, true)
    threadMenu = null
  }

  function threadMenuKeydown(event) {
    if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
      event.preventDefault()
      const items = [...event.currentTarget.querySelectorAll('button:not(:disabled)')]
      const index = items.indexOf(document.activeElement)
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length
      items[next]?.focus()
      return
    }
    if (event.key !== 'Escape' && event.key !== 'Tab') return
    event.preventDefault()
    const threadId = threadMenu?.threadId
    const fromTitle = threadMenu?.fromTitle
    closeThreadMenu()
    if (fromTitle) void tick().then(() => titleActionsButton?.focus())
    else focusThreadRow(threadId)
  }

  function focusMenuOnMount(element) {
    const bounds = element.getBoundingClientRect()
    element.style.left = `${Math.max(8, Math.min(threadMenu.x, window.innerWidth - bounds.width - 8))}px`
    element.style.top = `${Math.max(8, Math.min(threadMenu.y, window.innerHeight - bounds.height - 8))}px`
    element.querySelector('button')?.focus()
  }

  function menuFocusOut(event) {
    if (event.currentTarget.dataset.floatingMenu) return
    if (event.currentTarget.contains(event.relatedTarget)) return
    closeThreadMenu()
  }

  async function confirmDeleteThread(threadId) {
    if (deletePending) return
    deletePending = true
    const targets = deletingThreadIds.length ? deletingThreadIds : [threadId]
    for (const id of targets) {
      if (!(await chatController.deleteThread(id))) break
    }
    deletePending = false
    deleteFromTitle = false
    deletingThreadId = null
    deletingThreadIds = []
    clearThreadSelection()
  }

  function workspaceFrameWidth() {
    return workspace ? parseFloat(getComputedStyle(workspace).paddingRight) || 0 : 0
  }

  function availableRailWidth(occupant = railOccupant ?? 'artifacts') {
    // Reserve both outer edges and both panel gaps before sizing the rail.
    const bounds = railBounds(occupant)
    return Math.max(bounds.min, Math.min(bounds.max, (workspace?.clientWidth || window.innerWidth) - 4 * workspaceFrameWidth() - (sidebarCollapsed ? 0 : sidebarWidth) - minimumThreadWidth))
  }

  function availableSidebarWidth() {
    // Reserve both outer edges, the panel gaps, the thread minimum and the open rail before sizing the sidebar.
    const railOpen = railOccupant !== null
    const frames = railOpen ? 4 : 3
    return Math.max(SIDEBAR_MIN_WIDTH, Math.min(SIDEBAR_MAX_WIDTH, (workspace?.clientWidth || window.innerWidth) - frames * workspaceFrameWidth() - (railOpen ? artifactRailWidth : 0) - minimumThreadWidth))
  }

  function fitPanels() {
    fitSidebar()
    fitArtifactRail()
  }

  const dictationController = createDictationController({
    invoke: (...args) => tauri.invoke(...args),
    listen: (...args) => window.__TAURI__?.event?.listen(...args),
    readDraft: () => draft,
    updateDraft: (next) => { draft = next },
    blocked: () => !!active,
    onState: (state) => {
      dictation = state.status
      dictationCommandPending = state.commandPending
      dictationRequested = state.requested
      dictationFinishing = state.finishing
      dictationDraftSnapshot = state.draftSnapshot
      dictationTranscript = state.transcript
      if (state.status.state === 'modelNotInstalled') void openSpeechInstall()
      else speechInstallNotice = ''
    },
    onError: (message) => { dictationError = message },
    onInactive: () => voiceGesture.inactive(),
    onCancel: () => voiceGesture.cancel(),
    onFocus: () => tick().then(() => composer?.focus()),
  })

  function stopSpeechInstallPolling() {
    clearTimeout(speechInstallTimer)
    speechInstallTimer = undefined
  }

  async function readSpeechInstallStatus(epoch, pendingUntilTerminal = false) {
    try {
      const next = await tauri.invoke('parakeet_install_status')
      if (destroyed || epoch !== speechInstallEpoch) return
      speechInstallStatus = next
      if (next.state === 'installing') {
        speechInstallTimer = setTimeout(() => readSpeechInstallStatus(epoch, pendingUntilTerminal), 250)
      } else if (pendingUntilTerminal) {
        speechInstallPending = false
      }
      if (next.state === 'installed') settleSpeechInstall()
    } catch (_) {
      if (destroyed || epoch !== speechInstallEpoch) return
      speechInstallError = 'The speech model install state could not be checked. Try again.'
      speechInstallStatus = { state: 'failed' }
      if (pendingUntilTerminal) speechInstallPending = false
    }
  }

  // The card leaves once the model is on disk, and the composer hint carries the next step.
  function settleSpeechInstall() {
    speechInstallDismissed = true
    speechInstallNotice = installStateWords('installed')
  }

  function dismissSpeechInstall() {
    speechInstallDismissed = true
    void tick().then(() => composer?.focus())
  }

  function speechInstallKeydown(event) {
    if (event.key !== 'Escape') return
    event.preventDefault()
    event.stopPropagation()
    dismissSpeechInstall()
  }

  async function openSpeechInstall() {
    openComposerPanel('speech')
    speechInstallDismissed = false
    speechInstallNotice = ''
    if (speechInstallFacts || speechInstallPending) return
    speechInstallPending = true
    speechInstallError = ''
    try {
      const facts = await tauri.invoke('parakeet_install_facts')
      const status = await tauri.invoke('parakeet_install_status')
      if (destroyed) return
      speechInstallFacts = facts
      speechInstallStatus = status
      if (status.state === 'installing') {
        const epoch = ++speechInstallEpoch
        speechInstallTimer = setTimeout(() => readSpeechInstallStatus(epoch), 250)
      }
    } catch (_) {
      if (destroyed) return
      speechInstallError = 'The speech model install details could not be loaded. Try again.'
    } finally {
      if (!destroyed) speechInstallPending = false
    }
  }

  async function startSpeechInstall() {
    if (speechInstallPending || speechInstallStatus?.state === 'installing') return
    stopSpeechInstallPolling()
    const epoch = ++speechInstallEpoch
    speechInstallPending = true
    speechInstallError = ''
    try {
      const next = await tauri.invoke('parakeet_install_start')
      if (destroyed || epoch !== speechInstallEpoch) return
      speechInstallStatus = next
      if (next.state === 'installing') await readSpeechInstallStatus(epoch)
    } catch (_) {
      if (destroyed || epoch !== speechInstallEpoch) return
      speechInstallError = 'The speech model install could not start. Try again.'
      speechInstallStatus = { state: 'failed' }
    } finally {
      if (!destroyed && epoch === speechInstallEpoch) speechInstallPending = false
    }
  }

  async function cancelSpeechInstall() {
    if (speechInstallPending || speechInstallStatus?.state !== 'installing') return
    stopSpeechInstallPolling()
    const epoch = ++speechInstallEpoch
    speechInstallPending = true
    speechInstallError = ''
    try {
      const next = await tauri.invoke('parakeet_install_cancel')
      if (destroyed || epoch !== speechInstallEpoch) return
      speechInstallStatus = next
      if (next.state === 'installing') {
        const pollingEpoch = ++speechInstallEpoch
        await readSpeechInstallStatus(pollingEpoch, true)
      }
    } catch (_) {
      if (destroyed || epoch !== speechInstallEpoch) return
      speechInstallError = 'The speech model install could not be cancelled. Try again.'
      speechInstallPending = false
      const pollingEpoch = ++speechInstallEpoch
      await readSpeechInstallStatus(pollingEpoch)
    } finally {
      if (!destroyed && epoch === speechInstallEpoch) speechInstallPending = false
    }
  }

  function dictationBusy() {
    // Establish Svelte dependencies for the controller state read below.
    dictationCommandPending
    dictationFinishing
    dictation
    return dictationController.busy()
  }

  async function answerPermission(run, answer) {
    const gate = run.pendingPermission
    const current = permissionState(run)
    if (!gate || current?.pending) return
    const request = { runId: run.id, gateId: gate.gateId }
    permissionAnswer = { ...request, pending: true, error: '' }
    try {
      await tauri.invoke('chat_answer_permission', { ...request, answer })
      if (permissionAnswer?.runId === request.runId && permissionAnswer.gateId === request.gateId) {
        permissionAnswer = { ...request, pending: false, error: '' }
      }
    } catch (_) {
      if (permissionAnswer?.runId === request.runId && permissionAnswer.gateId === request.gateId) {
        permissionAnswer = {
          ...request,
          pending: false,
          error: 'Could not answer this request. Try again.',
        }
      }
    }
  }

  function permissionState(run) {
    const gate = run.pendingPermission
    return gate && permissionAnswer?.runId === run.id && permissionAnswer.gateId === gate.gateId
      ? permissionAnswer
      : null
  }

  function permissionValue(run) {
    const gate = run.pendingPermission
    const key = `${run.id}:${gate.gateId}`
    return Object.hasOwn(permissionValues, key)
      ? permissionValues[key]
      : gate.kind === 'editor' ? gate.prefill ?? '' : ''
  }

  function setPermissionValue(run, value) {
    const gate = run.pendingPermission
    permissionValues = { ...permissionValues, [`${run.id}:${gate.gateId}`]: value }
  }

  function permissionKeydown(event, run) {
    const gate = run.pendingPermission
    if (!gate || !permissionGateAction(event, gate.kind)) return
    event.preventDefault()
    event.stopPropagation()
    answerPermission(run, { type: gate.kind, value: permissionValue(run) })
  }

  function stopDictation(cancelled = false) {
    return dictationController.stop(cancelled)
  }

  function startDictation() {
    return dictationController.start()
  }

  const voiceGesture = createVoiceGesture({
    start: startDictation,
    stop: stopDictation,
    readRequested: () => dictationRequested,
    readStatus: () => dictation,
    busy: dictationBusy,
    signedIn: () => workspaceMode(),
    hasActiveRun: () => !!active,
  })
  const { pointerDown: voicePointerDown, pointerEnd: voicePointerEnd, keyDown: voiceKeyDown, keyUp: voiceKeyUp, click: voiceClick, globalShortcut: globalVoiceShortcut } = voiceGesture

  const voiceShortcutManager = createVoiceShortcutManager({
    register,
    unregister,
    storage: localStorage,
    onShortcut: globalVoiceShortcut,
    onState: (state) => {
      globalVoiceShortcutValue = state.shortcut
      globalVoiceChanging = state.changing
      globalVoiceRegistered = state.registered
      globalVoiceError = state.error
    },
  })

  function changeVoiceShortcut(next) {
    return voiceShortcutManager.change(next)
  }

  let workspaceDirectory = $state('')
  $effect(() => { currentThreadId; selectedProject; workspaceDirectory = ''; untrack(closeMentions) })
  let mention = $state(null)
  let mentionFiles = $state([])
  let mentionSelected = $state(0)
  let mentionLoading = $state(false)
  let mentionError = $state('')
  let mentionVersion = 0
  let mentionTimer
  let composerScrollTop = $state(0)
  let composerScrollLeft = $state(0)
  let composerTextWidth = $state(0)
  const draftLinks = $derived([...new Set(composerParts(draft).filter((part) => part.type === 'link').map((part) => part.text))])
  function closeMentions() {
    clearTimeout(mentionTimer); mentionVersion += 1; mention = null; mentionFiles = []; mentionLoading = false
  }
  function updateMention() {
    clearTimeout(mentionTimer)
    const version = ++mentionVersion
    mention = !active && composer ? mentionQuery(composer.value, composer.selectionStart) : null
    mentionSelected = 0; mentionError = ''; mentionFiles = []
    if (!mention) { mentionLoading = false; return }
    openComposerPanel('mentions')
    mentionLoading = true
    const query = mention.query
    mentionTimer = setTimeout(async () => {
      try {
        const files = await tauri.invoke('chat_search_files', { query, ...(currentThreadId ? { threadId: currentThreadId } : {}), ...(selectedProject ? { projectId: selectedProject } : {}), ...(workspaceDirectory ? { directory: workspaceDirectory } : {}) })
        if (version === mentionVersion) mentionFiles = files
      } catch (error) { if (version === mentionVersion) mentionError = String(error?.message ?? error) }
      finally { if (version === mentionVersion) mentionLoading = false }
    }, 120)
  }
  async function chooseMention(file) {
    const current = mention
    const before = draft
    closeMentions()
    await addFiles([file.path])
    if (!current || draft !== before || !selectedFiles.some((f) => f.path === file.path)) return
    selectedFiles = selectedFiles.map((item) => item.path === file.path ? { ...item, referenceName: file.relativePath } : item)
    const inserted = insertMention(draft, current, file.relativePath)
    draft = inserted.text
    await tick()
    composer?.focus(); composer?.setSelectionRange(inserted.cursor, inserted.cursor)
  }
  function syncComposerScroll() {
    composerScrollTop = composer?.scrollTop ?? 0
    composerScrollLeft = composer?.scrollLeft ?? 0
    composerTextWidth = composer?.clientWidth ?? 0
  }
  $effect(() => { if (!draft || active) untrack(closeMentions) })

  function composerInput(event) {
    composerInputDraft = event.currentTarget.value
    updateMention()
  }

  function focusComposerOnMount(node) {
    if (!wasInWorkspace && active?.phase !== 'resuming') {
      wasInWorkspace = true
      node.focus()
    }
  }

  // §4: the input grows with the draft to a ten-line cap, then scrolls.
  // Measuring needs the textarea collapsed first, and every step of that
  // resizes the thread, which lets the browser clamp its scrollTop. Put the
  // transcript back where it was. Keep it at the bottom while pinned. Otherwise,
  // keep it where the reader left it so composer growth never scrolls it.
  // `reveal` is set by the draft path only: new text should be brought into
  // view, but a mere relayout must leave the reader wherever they were.
  function syncComposerHeight(reveal = false) {
    if (!composer) return
    syncComposerScroll()
    const styles = getComputedStyle(composer)
    const threadScrollTop = thread?.scrollTop
    composer.style.height = 'auto'
    const { height, capped } = composerHeight({
      contentHeight: composer.scrollHeight,
      lineHeight: parseFloat(styles.lineHeight),
      padding: parseFloat(styles.paddingTop) + parseFloat(styles.paddingBottom),
    })
    if (height === null) {
      composer.style.removeProperty('height')
      composer.style.removeProperty('overflow-y')
    } else {
      composer.style.height = `${height}px`
      composer.style.overflowY = capped ? 'auto' : 'hidden'
      // Past the cap, a streamed transcript or transform result lands below the
      // fold after a programmatic write. The user then watches their words vanish.
      // Typed input needs no help: the browser keeps the caret in view.
      if (reveal && capped) composer.scrollTop = composer.scrollHeight
    }
    syncComposerScroll()
    if (thread) {
      const restored = pinned ? thread.scrollHeight - thread.clientHeight : threadScrollTop
      if (thread.scrollTop !== restored) thread.scrollTop = restored
      // The restore fires a scroll event; leave the follow state matching it so
      // handleThreadScroll does not read the correction as an upward scroll.
      transcriptController.syncScrollTop(thread.scrollTop)
    }
  }

  // Keyed off `draft` rather than the input event so every programmatic write
  // resizes too: streamed dictation transcripts, the polish and transform
  // results, the Esc restore in stopDictation(true), the Try again retry, and
  // send()/queue() clearing the draft back to the resting height. Untracked so
  // the follow state it reads cannot re-enter. Resizing must answer to the
  // draft alone, never to a scroll already in flight.
  $effect(() => {
    draft
    if (composer) untrack(() => {
      const typed = draft === composerInputDraft
      composerInputDraft = undefined
      syncComposerHeight(!typed)
    })
  })

  $effect(() => {
    selectedFiles
    if (composer) untrack(syncComposerHeight)
  })

  // The composer also rewraps when only its width changes, and most of those
  // never touch the window: ⌘J opening the artifact rail, the rail separator
  // being dragged or arrow-keyed, the sidebar collapsing. A height measured at
  // the old width would clip the draft with no scrollbar to reach it, so watch
  // the layout rather than the window. The action row is the box to observe:
  // it spans the same width as the input but is the one part of the composer
  // whose size we never set ourselves, so the callback cannot resize its own
  // target. Observing the textarea makes Chromium report "ResizeObserver loop
  // completed with undelivered notifications" all through a rail drag.
  $effect(() => {
    if (!composerRow || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => untrack(syncComposerHeight))
    observer.observe(composerRow)
    return () => observer.disconnect()
  })

  $effect(() => {
    if (!composerBox || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => { composerBoxHeight = composerBox.offsetHeight })
    observer.observe(composerBox)
    return () => observer.disconnect()
  })

  $effect(() => {
    messages
    followNewContent()
  })

  $effect(() => {
    const inWorkspace = workspaceMode() && onboarding.name === 'complete'
    const hasConversation = currentThreadId !== null || messages.some(({ role }) => role === 'user')
    void windowTitle.set(inWorkspace && hasConversation ? currentThreadTitle : undefined)
  })

  $effect(() => {
    if (!workspaceMode() || onboarding.name !== 'complete') { browserPanel = null; closeRail() }
  })

  function workspaceMode() {
    return auth.name === 'signed-in' || auth.name === 'local' || (auth.name === 'signing-in' && signInFromLocal)
  }

  // A window outside a workspace drives no run, so the desktop drops the active one.
  // The next workspace entry restores history and rejoins whatever the runtime runs.
  $effect(() => {
    if (!workspaceMode()) active = null
  })

  $effect(() => {
    if (auth.name !== 'signed-in') entitlementToast.clear()
  })

  $effect(() => {
    const inWorkspace = workspaceMode() && onboarding.name === 'complete' && desktopClientStatus
      && !backgroundServiceNoticeVisible
    if (inWorkspace && !wasInWorkspace && active?.phase !== 'resuming' && pairingRequests.length === 0 && composer) {
      wasInWorkspace = true
      composer.focus()
    } else if (!inWorkspace) {
      wasInWorkspace = false
    }
  })

  $effect(() => {
    const transcript = messages
    untrack(() => transcriptController.trackParallelTools(transcript))
  })

  // The browser did not open, or the user closed it. The link opens it again.
  function openSignInLink(event) {
    event.preventDefault()
    if (auth.name !== 'signing-in' || !auth.link) return
    void openUrl(auth.link).catch(() => {})
  }

  async function run(action) {
    if (!featureFlags.cloud) return false
    const version = ++authRequestVersion
    const command = {
      status: 'auth_status',
      'sign-in': 'auth_sign_in',
      'sign-out': 'auth_sign_out',
    }[action]

    if (action === 'sign-in') auth = waitingState()
    try {
      const status = await tauri.invoke(command)
      if (version !== authRequestVersion) return
      auth = statusState(status)
      if (action === 'sign-out') { artifactsOpen = false; projectsOpen = false; agentsOpen = false; agentsExpanded = false; artifactsExpanded = false }
      if (auth.name === 'signed-in') await chatController.loadHistory()
      return true
    } catch (err) {
      if (version === authRequestVersion) auth = errorState(action, err)
      return false
    }
  }

  async function refreshInventory() {
    const version = ++inventoryRequestVersion
    try {
      const next = await tauri.invoke('local_mode_provider_inventory')
      if (version === inventoryRequestVersion) {
        inventory = next
        if (accountStatus === inventoryError) accountStatus = ''
        inventoryError = ''
      }
      return true
    } catch (_) {
      if (version === inventoryRequestVersion) {
        inventoryError = 'Muniment cannot read provider settings. Restart the app to retry.'
        accountStatus = inventoryError
      }
      return false
    }
  }

  // A saved session launches signed in. Any other shell past onboarding enters
  // local mode on its own, including the shell a sign-out leaves behind. The
  // first run keeps its own entry, and a failed entry keeps the sign-in screen.
  $effect(() => {
    if (onboarding.name === 'complete' && auth.name === 'signed-out' && !localEntryPending && !localEntryError) {
      void enterLocalMode()
    }
  })

  async function enterLocalMode() {
    if (auth.name !== 'signed-out' || localEntryPending) return
    localEntryPending = true
    authRequestVersion += 1
    localEntryError = ''
    try {
      await tauri.invoke('local_mode_enter')
      markerStartupLocalMode = true
      auth = { name: 'local', subject: null }
      await refreshInventory()
      await chatController.loadHistory()
      return true
    } catch (_) {
      localEntryError = 'Local mode could not start. Try again.'
      return false
    } finally {
      localEntryPending = false
    }
  }

  async function signIn() {
    if (!featureFlags.cloud || localEntryPending || (auth.name !== 'signed-out' && auth.name !== 'local')) return
    settingsOpen = false
    signInFromLocal = auth.name === 'local'
    if (auth.name === 'local') {
      try {
        await tauri.invoke('local_mode_leave')
        markerStartupLocalMode = false
      } catch (_) {
        accountStatus = 'Cloud sign-in could not start. Try again.'
        return
      }
    }
    signInPending = run('sign-in')
  }

  function startWorkspace() {
    markerStartupReady = tauri.invoke('local_mode_status')
    startupReady = markerStartupReady.then(async (active) => {
      markerStartupLocalMode = active
      if (active) {
        auth = { name: 'local', subject: null }
        void refreshInventory()
        await chatController.loadHistory()
      } else if (!featureFlags.cloud) {
        auth = { name: 'signed-out' }
        await enterLocalMode()
      } else {
        await run('status')
      }
      return true
    }).catch(() => {
      auth = { name: 'signed-out' }
      localEntryError = 'Local mode could not be checked. Try again.'
      return false
    })
  }

  onMount(() => {
    const agentTimer = setInterval(() => { if (auth.name === 'local' || auth.name === 'signed-in') { void refreshAgents(); void refreshCreations() } }, 15000)
    return () => clearInterval(agentTimer)
  })

  onMount(() => {
    let pairingUnlisten
    let registrationRetryUnlisten
    let desktopClientUnlisten
    let launcherUnlisten
    const readDesktopClientStatus = () => {
      const version = desktopClientStatusVersion
      return tauri?.invoke('attach_listener_status').then((status) => {
        if (version === desktopClientStatusVersion) {
          const connectionRecovered = desktopClientStatus?.connected !== true && status?.connected === true
          applyDesktopClientStatus(status)
          refreshAuthAfterStartup(connectionRecovered)
        }
      }).catch(() => {
        if (version === desktopClientStatusVersion) {
          applyDesktopClientStatus({ connected: false, supervisor_running: false })
        }
        console.error('Desktop client status failed.')
      })
    }
    const refreshAuthAfterStartup = (connectionRecovered) => {
      if (!featureFlags.cloud || !connectionRecovered || markerStartupLocalMode === true) return
      if (markerStartupLocalMode === false) {
        if (!destroyed && !localEntryPending && auth.name !== 'local') void run('status')
        return
      }
      void markerStartupReady.then((localModeActive) => {
        if (!destroyed && !localEntryPending && !localModeActive && auth.name !== 'local') void run('status')
      }).catch(() => {})
    }
    const startDesktopClientStatus = async () => {
      try {
        const stop = await window.__TAURI__?.event?.listen('desktop-client-status-changed', ({ payload }) => {
          const connectionRecovered = desktopClientStatus?.connected !== true && payload?.connected === true
          desktopClientStatusVersion += 1
          applyDesktopClientStatus(payload)
          refreshAuthAfterStartup(connectionRecovered)
        })
        if (destroyed) stop?.()
        else desktopClientUnlisten = stop
      } catch {
        desktopClientUnlisten = undefined
        console.error('Desktop client listener registration failed.')
      }
      await readDesktopClientStatus()
    }
    if (featureFlags.cloud) window.__TAURI__?.event?.listen('auth-registration-retry', ({ payload }) => {
      if (auth.name === 'signing-in') auth = registrationRetryState(payload?.delay_seconds)
    }).then((stop) => {
      if (destroyed) stop()
      else registrationRetryUnlisten = stop
    }).catch(() => {
      registrationRetryUnlisten = undefined
      console.error('Registration retry status failed.')
    })
    window.__TAURI__?.event?.listen('attach-pairing-requested', ({ payload }) => {
      pairingRequests = [...pairingRequests, {
        challenge: payload?.challenge,
        claimedKind: boundedAttachClaim(payload?.claimed_kind),
        claimedVersion: boundedAttachClaim(payload?.claimed_version),
        workspace: boundedAttachWorkspace(payload?.workspace),
        scopes: Array.isArray(payload?.scopes) ? payload.scopes : [],
      }]
      const appWindow = getCurrentWindow()
      void appWindow.isFocused().then((focused) => {
        if (!focused) return appWindow.requestUserAttention(UserAttentionType.Informational)
      }).catch(() => console.error('Pairing attention request failed.'))
    }).then((stop) => {
      if (destroyed) stop()
      else pairingUnlisten = stop
    }).catch(() => {
      pairingUnlisten = undefined
      console.error('Pairing decision failed.')
    })
    if (tauri) {
      void startDesktopClientStatus()
      startWorkspace()
      void backgroundServiceNotice.start()
      chatController.start()
      void listenForLauncher({
        listen: (...args) => window.__TAURI__.event.listen(...args),
        emitTo: (...args) => window.__TAURI__.event.emitTo(...args),
        ready: async () => { await startupReady; return !destroyed && workspaceMode() },
        send: (text) => chatController.sendNewThread(text),
      }).then((stop) => {
        if (destroyed) { stop(); return }
        launcherUnlisten = stop
        // Register only after the main window can receive a launcher message.
        return tauri.invoke('launcher_register')
      }).catch((error) => console.error('The launcher could not start. Restart the app.', error))
      if (featureFlags.cloud) entitlementToast.start()
      voiceShortcutManager.start()
    }
    const shortcuts = (event) => {
      if (deletingThreadId || event.defaultPrevented) return
      const sidebarTarget = event.target?.closest?.('#sidebar, .thread-menu')
      const editing = event.target?.closest?.('input,textarea,[contenteditable="true"]')
      if (sidebarTarget && !editing && !active && !threadSwitching) {
        if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'a') {
          event.preventDefault()
          selectedThreadIds = new Set(shortcutThreads.map(t => t.threadId).filter(Boolean))
          return
        }
        if (selectedThreadIds.size && ['Delete','Backspace'].includes(event.key)) {
          event.preventDefault()
          askToDeleteThread([...selectedThreadIds][0])
          return
        }
        if (event.key === 'Escape') clearThreadSelection()
      }
      if (auth.name === 'signing-in' && event.key === 'Escape') { event.preventDefault(); void cancelSignIn(); return }
      if (auth.name === 'signing-in') return
      const sizeStep = typeSizeShortcutStep(event)
      if (sizeStep !== null) {
        event.preventDefault()
        typeSizeShortcutPressed(sizeStep)
        return
      }
      const rowPosition = threadRowShortcutPosition(event)
      if (workspaceMode() && onboarding.name === 'complete' && rowPosition !== null) {
        event.preventDefault()
        if (sidebarCollapsed || active || threadSwitching) return
        const threadId = shortcutThreads[rowPosition - 1]?.threadId
        if (!threadId || threadId === currentThreadId) return
        projectsOpen = false; agentsOpen = false; artifactsOpen = false
        void chatController.openThread(threadId)
        return
      }
      if (workspaceMode() && onboarding.name === 'complete' && isNewThreadShortcut(event)) {
        event.preventDefault()
        void newSidebarThread()
        return
      }
      if (workspaceMode() && onboarding.name === 'complete' && isArtifactRailShortcut(event)) {
        event.preventDefault()
        toggleArtifactRail()
        return
      }
      if (featureFlags.companyRecord && workspaceMode() && onboarding.name === 'complete' && isRecordPanelShortcut(event)) {
        event.preventDefault()
        // ⌘K on a maximized record panel returns the sidebar and the thread first.
        if ((recordPanelOpen || filePanelOpen) && recordMaximized) toggleRecordMaximized()
        else toggleRecordPanel()
        return
      }
      if (workspaceMode() && onboarding.name === 'complete' && isSidebarShortcut(event)) {
        event.preventDefault()
        toggleSidebar()
        return
      }
      if (workspaceMode() && onboarding.name === 'complete' && isSettingsShortcut(event)) {
        event.preventDefault()
        settingsShortcutPressed()
        return
      }
      if (event.key === 'Escape' && (pickerOpen || capacityOpen || (dictation.state === 'modelNotInstalled' && !speechInstallDismissed))) { openComposerPanel(null); event.preventDefault(); return }
      if (event.key === 'Escape' && projectsOpen) { projectsOpen = false; event.preventDefault() }
      if (event.key === 'Escape' && artifactsOpen) { artifactsOpen = false; event.preventDefault() }
      if (event.key === 'Escape' && browserPanel) { browserPanel = null; event.preventDefault() }
      if (event.key === 'Escape' && railOccupant !== null) {
        event.preventDefault()
        if ((recordPanelOpen || filePanelOpen) && recordMaximized) toggleRecordMaximized()
        else closeRail()
      }
      if (event.key === 'Escape' && (dictationRequested || isDictationActive(dictation) || dictationFinishing)) {
        event.preventDefault()
        stopDictation(true)
        return
      }
    }
    document.addEventListener('keydown', shortcuts)
    window.addEventListener('resize', fitPanels)
    window.addEventListener('resize', agentResize.fit)
    window.addEventListener('resize', workspaceResize.fit)
    const refreshRecord = () => { if (recordPanelOpen) recordRefresh += 1 }
    window.addEventListener('focus', refreshRecord)
    let stopDragDrop
    if (tauri) getCurrentWebview().onDragDropEvent(({ payload }) => {
        if (!workspaceMode() || active) {
          draggingFiles = false
          return
        }
        if (payload.type === 'over') draggingFiles = true
        if (payload.type === 'leave') draggingFiles = false
        if (payload.type === 'drop') {
          draggingFiles = false
          addFiles(payload.paths)
        }
      }).then((stop) => {
        if (destroyed) stop()
        else stopDragDrop = stop
      }).catch(() => {
        stopDragDrop = undefined
        console.error('File drop listener registration failed.')
      })
    return () => {
      closeMentions()
      destroyed = true
      speechInstallEpoch += 1
      stopSpeechInstallPolling()
      dictationController.cleanup()
      chatController.cleanup()
      entitlementToast.cleanup()
      backgroundServiceNotice.cleanup()
      pairingUnlisten?.()
      registrationRetryUnlisten?.()
      desktopClientUnlisten?.()
      launcherUnlisten?.()
      voiceGesture.cleanup()
      transcriptController.cleanup()
      stopDragDrop?.()
      voiceShortcutManager.cleanup()
      document.removeEventListener('keydown', shortcuts)
      window.removeEventListener('resize', fitPanels)
      window.removeEventListener('resize', agentResize.fit)
      window.removeEventListener('resize', workspaceResize.fit)
      window.removeEventListener('focus', refreshRecord)
    }
  })

  async function decidePairing(approve) {
    const request = pairingRequests[0]
    if (!request) return
    try {
      await tauri.invoke('attach_pairing_decide', { challenge: request.challenge, approve })
    } catch {
      console.error('Pairing decision failed.')
    } finally {
      pairingRequests = pairingRequests.slice(1)
    }
  }

  async function chooseFiles() {
    if (active) return
    submitError = ''
    let picked
    try {
      picked = await open({ multiple: true, directory: false })
    } catch (_) {
      submitError = 'Files could not be selected. Try again.'
      return
    }
    if (!picked) return
    const paths = Array.isArray(picked) ? picked : [picked]
    await addFiles(paths)
  }

  async function addFiles(paths) {
    const known = new Set(selectedFiles.map(({ path }) => path))
    const additions = []
    try {
      for (const path of paths) {
        if (known.has(path)) continue
        known.add(path)
        additions.push({ path, ...await tauri.invoke('chat_file_metadata', { path }) })
      }
    } catch (_) {
      submitError = 'One or more selected files could not be added. Check the files and try again.'
      return
    }
    selectedFiles = [...selectedFiles, ...additions]
  }

  async function openFirstRunModelSettings() {
    if (retryStartup) startWorkspace()
    retryStartup = await startupReady === false
    if (retryStartup) throw firstRunError(tauri, 'startup')
    if (auth.name === 'error' && auth.retry === 'status' && await run('status') === false) {
      throw firstRunError(tauri, 'sessionStatus')
    }
    if (auth.name === 'signed-out' && await enterLocalMode() === false) {
      throw firstRunError(tauri, 'localMode')
    }
    if (!workspaceMode() || desktopClientStatus?.connected !== true) {
      throw firstRunError(tauri, 'runtime')
    }
    onboarding = { name: 'complete', homePath: onboarding.homePath }
    // SPEC.md: the first Send opens Settings → Models when no source answers.
    openSettings('models')
  }

  function keydown(event) {
    if (composerExtensions?.handleKey(event)) return
    if (mention && !event.isComposing) {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeMentions(); return }
      if (['ArrowDown', 'ArrowUp'].includes(event.key) && mentionFiles.length) {
        event.preventDefault()
        mentionSelected = (mentionSelected + (event.key === 'ArrowDown' ? 1 : -1) + mentionFiles.length) % mentionFiles.length
        void tick().then(() => document.getElementById(`file-mention-${mentionSelected}`)?.scrollIntoView({ block: 'nearest' }))
        return
      }
      if (event.key === 'Enter' || (event.key === 'Tab' && mentionFiles.length)) {
        event.preventDefault()
        if (mentionFiles[mentionSelected]) void chooseMention(mentionFiles[mentionSelected])
        return
      }
    }
    const action = composerAction(event, draft, active)
    if (action) {
      event.preventDefault()
      action === 'submit' ? chatController.send() : chatController.queue('steer')
    }
  }

  // ADR 0012: the runtime rejects a new run while its upgrade is pending. The
  // shell holds send, resume and queue until a compatible welcome clears the flag.
  function runtimeUpgradePending() {
    return desktopClientStatus?.runtime_upgrade_pending === true
  }

  function sendDisabled() {
    return runtimeUpgradePending() || !draft.trim() || active?.phase === 'resuming' || active?.id === 'pending' || (!active && (dictationBusy() || threadSwitching))
  }

  // The band's one action control: an up arrow that sends while the draft has
  // text, and a stop square while a reply is in flight. Enter steers in flight.
  function composerActionInactive() {
    return active ? active.id === 'pending' || active.phase === 'resuming' : sendDisabled() || modelChoicePending || !!retryReview?.chooseModel
  }

  function composerActionClick() {
    if (composerActionInactive()) return
    active ? chatController.cancel() : chatController.send()
  }
</script>

{#snippet projectThreadList(projectId = null)}
            <ul class="thread-list" aria-label={projectId ? `${projectCatalog.projects[projectId]} threads` : "Threads"}>
              {#if showFreshThread && selectedProject === projectId}
                <li class="thread-row active-thread" data-fresh-thread aria-current="true" aria-keyshortcuts={threadRowShortcut(freshThreadPosition + 1)}><span></span><div class="thread-row-title">{currentThreadTitle}</div></li>
              {/if}
            {#each threadGroups.filter((group) => group.name !== 'Pinned').map(group => ({ ...group, threads: group.threads.filter(thread => (projectCatalog.threads[thread.threadId] || null) === projectId) })).filter(group => group.threads.length) as group (group.name)}
                {#if group.name !== 'Threads'}<li class="side-group" role="presentation"><h3>{group.name}</h3></li>{/if}
              {#each group.threads as summary (summary.threadId)}
                {@render sidebarThread(summary)}
              {/each}
              {/each}
            </ul>
{/snippet}

{#snippet sidebarThread(summary)}
                {@const title = summary.title || 'New thread'}
                {@const current = !freshThread && summary.threadId === (currentThreadId ?? threadSummaries[0]?.threadId)}
                {@const rowTitle = current ? summary.title || currentThreadTitle : title}
                {@const rowPosition = shortcutThreads.findIndex((item) => item.threadId === summary.threadId) + 1}
                {@const selected = selectedThreadIds.has(summary.threadId)}
                <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                <li class="thread-record" class:selected oncontextmenu={(event) => openThreadMenu(event, summary.threadId)}>
                  {#if rowRename?.threadId === summary.threadId}
                    <form class="row-rename" onsubmit={(event) => { event.preventDefault(); void saveSidebarName() }}>
                      <input aria-label="Thread name" maxlength="80" bind:this={rowRenameInput} bind:value={rowRename.title} onkeydown={sidebarNameKeydown} disabled={rowRenamePending} />
                      <button type="submit" disabled={rowRenamePending || !rowRename.title.trim()}>Save</button>
                      <button type="button" disabled={rowRenamePending} onclick={() => { rowRename = null; focusThreadRow(summary.threadId) }}>Cancel</button>
                    </form>
                  {:else if current}
                    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
                    <div class="thread-row active-thread" tabindex="-1" class:selected data-thread-id={summary.threadId} aria-current="true" aria-keyshortcuts={rowPosition <= 9 ? threadRowShortcut(rowPosition) : undefined} onclick={(event) => currentRowClick(event, summary.threadId)}><span></span><div class="thread-row-title">{rowTitle}</div><time datetime={summary.updatedAt}>{relativeTime(summary.updatedAt)}</time></div>
                  {:else}
                    <button class="thread-row" class:selected data-thread-id={summary.threadId} data-selected={selected ? 'true' : undefined} aria-keyshortcuts={rowPosition <= 9 ? threadRowShortcut(rowPosition) : undefined} aria-disabled={active ? 'true' : undefined} onclick={(event) => threadRowClick(event, summary.threadId)} onkeydown={(event) => threadRowKeydown(event, summary.threadId)}><span></span><div class="thread-row-title">{rowTitle}</div><time datetime={summary.updatedAt}>{relativeTime(summary.updatedAt)}</time></button>
                  {/if}
                  {#if deletingThreadId !== summary.threadId && rowRename?.threadId !== summary.threadId}
                    <button type="button" class="quiet thread-actions" aria-label={`Actions for ${rowTitle}`} aria-haspopup="menu" aria-expanded={threadMenu?.threadId === summary.threadId} onclick={(event) => openThreadMenu(event, summary.threadId)}><LucideIcon name="ellipsis-vertical" variant="action" size={14} /></button>
                  {/if}

                </li>
{/snippet}

<main class:onboarding-active={tauri && onboarding.name !== 'complete'}>
  {#if !workspaceMode() || onboarding.name !== 'complete'}
    <div class="lockup">
      <GraphMark size={160} />
      {#if onboarding.name === 'complete' && (auth.name === 'signed-out' || auth.name === 'signing-in')}
        <h1 class="name">muniment</h1>
      {:else}
        <span class="name">muniment</span>
      {/if}
    </div>
    <p class="meta">shell v{version}</p>
  {/if}

  {#if tauri}
    <Onboarding {tauri} bind:onboarding bind:draft onready={openFirstRunModelSettings} />
    {#if backgroundServiceNoticeVisible}
      <section class="auth-state" aria-live="polite" data-testid="runtime-notice">
        <p class="record error-record">{runtimeNotice.text}</p>
        <button type="button" disabled={runtimeNotice.busy} onclick={() => backgroundServiceNotice.retry()}>{runtimeNotice.control}</button>
      </section>
    {:else if onboarding.name === 'complete'}
      {#if featureFlags.cloud && (auth.name === 'signed-out' || auth.name === 'signing-in')}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <section role="region" aria-label="Sign in" class="auth-state" class:sign-in-overlay={auth.name === 'signing-in' && signInFromLocal} onkeydown={(event) => { if (event.key === 'Escape') { event.preventDefault(); void cancelSignIn() } }}>
        <p class="support" aria-live="polite">{auth.name === 'signing-in' ? auth.message : 'Sign in for cloud features, or use local mode.'}</p>
        {#if auth.name === 'signing-in' && auth.link}
          <a class="sign-in-link" data-testid="sign-in-link" href={auth.link} target="_blank" rel="noopener noreferrer" onclick={openSignInLink}>Open the sign-in page</a>
        {/if}
        <div class="auth-actions">
          <button class="primary" class:inactive={auth.name === 'signing-in' || localEntryPending} disabled={localEntryPending} aria-disabled={auth.name === 'signing-in' || localEntryPending ? 'true' : undefined} onclick={signIn}>Sign in</button>
          <button bind:this={signInCancelControl} disabled={localEntryPending} onclick={auth.name === 'signing-in' ? cancelSignIn : enterLocalMode}>{auth.name === 'signing-in' ? 'Cancel sign-in' : 'Use local mode'}</button>
        </div>
        {#if localEntryError}<p class="record error-record" role="alert">{localEntryError}</p>{/if}
      </section>
    {/if}
    {#if !featureFlags.cloud && auth.name === 'signed-out'}
      <section class="auth-state" aria-label="Local workspace">
        {#if localEntryError}<p class="record error-record" role="alert">{localEntryError}</p>{/if}
        <button type="button" disabled={localEntryPending} onclick={enterLocalMode}>{localEntryPending ? 'Opening workspace…' : 'Open workspace'}</button>
      </section>
    {/if}
    {#if workspaceMode() && desktopClientStatus}
      <section class="workspace" inert={auth.name === 'signing-in' || !!deletingThreadId} data-testid={auth.name === 'local' ? 'local-mode' : undefined} class:macos={macOS} class:sidebar-collapsed={sidebarCollapsed} class:rail-open={railOccupant !== null} class:agents-open={agentsOpen || artifactsOpen || projectsOpen} class:tools-open={!!browserPanel} class:agent-profile-open={agentProfileOpen && !!profileAgent && !agentsOpen} class:record-maximized={(recordPanelOpen || filePanelOpen) && recordMaximized} class:artifact-resizing={artifactRailPointer !== undefined || workspacePanelPointer !== undefined} class:sidebar-resizing={sidebarPointer !== undefined} style:--workspace-panel-width={`${workspacePanelWidth}px`} style:--agent-panel-width={`${agentPanelWidth}px`} style:--artifact-rail-width={`${artifactRailWidth}px`} style:--sidebar-column={`${sidebarCollapsed ? 0 : sidebarWidth}px`} bind:this={workspace}>
        <header class="titlebar" data-tauri-drag-region>
          <div class="titlebar-sidebar" data-tauri-drag-region>
            <button type="button" class="quiet side-toggle" aria-controls="sidebar" aria-expanded={!sidebarCollapsed} aria-keyshortcuts={sidebarKeyShortcut} aria-label={`${sidebarCollapsed ? 'Expand' : 'Collapse'} sidebar`} onclick={toggleSidebar}>
              <LucideIcon name={sidebarCollapsed ? 'panel-left-open' : 'panel-left-close'} />
            </button>
          </div>
          <div class="titlebar-thread" data-tauri-drag-region>
            {#if profileAgent}<button class="quiet agent-chat-title" aria-label={`Open profile for ${profileAgent.name}`} onclick={revealAgentProfile}><AgentAvatar agent={profileAgent} size={20} /><span>{profileAgent.name}</span></button>{/if}
            {#if !profileAgent}
            {#if editingThreadTitle}
              <input class="thread-title" aria-label="Thread name" maxlength="160" bind:this={threadTitleInput} value={threadTitleDraft} oninput={limitThreadTitle} onkeydown={threadTitleKeydown} onblur={commitThreadTitle}>
            {:else}
              <h1 class="thread-title-heading" aria-label={currentThreadTitle} data-tauri-drag-region><RowControl kind="thread-title" aria-label="Rename thread" disabled={!currentThreadId || freshThread} bind:element={threadTitleButton} onclick={() => editThreadTitle(currentThreadTitle)} onkeydown={threadTitleButtonKeydown}>{currentThreadTitle}</RowControl></h1>
            {/if}
            <button type="button" class="quiet title-thread-actions" aria-label="Thread actions" aria-haspopup="menu" aria-expanded={!!threadMenu?.fromTitle} disabled={!currentThreadId || freshThread || threadSwitching} bind:this={titleActionsButton} onclick={(event) => openThreadMenu(event, currentThreadId, true)}><LucideIcon name="ellipsis" variant="action" size={14} /></button>
            {/if}

            <span class="title-spacer" data-tauri-drag-region></span>
            <span class="update-slot" data-tauri-drag-region aria-hidden="true"></span>
            {#if featureFlags.companyRecord}<RowControl kind="record-toggle" aria-controls="record-panel" aria-expanded={recordPanelOpen} aria-keyshortcuts={recordShortcut} aria-label={`${recordPanelOpen ? 'Close' : 'Open'} record panel`} onclick={toggleRecordPanel}>Record <kbd>{shortcutDisplayLabel(recordShortcut)}</kbd></RowControl>{/if}
            <WorkspaceMenu selected={browserPanel} onselect={showBrowser} onopenchange={value => toolsMenuOpen = value} />
          </div>
        </header>
                  {#if threadMenu}
                    {@const summary = threadSummaries.find((item) => item.threadId === threadMenu.threadId) || { threadId: threadMenu.threadId, title: currentThreadTitle }}
                    {@const rowTitle = summary.title || 'New thread'}
                    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                    <div class="thread-menu" use:floatingMenu role="menu" tabindex="-1" aria-label={`${rowTitle} actions`} style:left={`${threadMenu.x}px`} style:top={`${threadMenu.y}px`} onkeydown={threadMenuKeydown} onfocusout={menuFocusOut} use:focusMenuOnMount>
                      <button type="button" role="menuitem" onclick={() => { if (threadMenu.fromTitle) { closeThreadMenu(); editThreadTitle(currentThreadTitle) } else renameSidebarThread(summary) }}>Rename</button>
                      <button type="button" role="menuitem" onclick={() => setThreadOrganization(summary.threadId, 'pinned')}>{threadOrganization[summary.threadId]?.pinned ? 'Unpin' : 'Pin'}</button>
                      <button type="button" role="menuitem" onclick={() => setThreadOrganization(summary.threadId, 'archived')}>{threadOrganization[summary.threadId]?.archived ? 'Restore' : 'Archive'}</button>
                      <button type="button" role="menuitem" aria-label={deleteLabel(summary.threadId, rowTitle)} disabled={!!active || threadSwitching} onclick={() => askToDeleteThread(summary.threadId)}>{deletionTargets(summary.threadId).length > 1 ? deleteLabel(summary.threadId, rowTitle) : 'Delete'}</button>
                    </div>
                  {/if}
        <aside data-panel="sidebar" id="sidebar" class="sidebar">
          {#if !sidebarCollapsed}
            <div class="side-top">
              <button class="side-action new-thread" aria-label="New thread" aria-keyshortcuts={newThreadKeyShortcut} disabled={!!active || threadSwitching} onclick={() => newSidebarThread()}>
                <LucideIcon name="square-pen" /><span>New thread</span><kbd>{shortcutDisplayLabel(newThreadKeyShortcut)}</kbd>
              </button>
            </div>
            {#if selectedThreadIds.size}<div class="selection-bar"><span role="status">{selectedThreadIds.size} selected</span><button type="button" disabled={!!active || threadSwitching} onclick={() => askToDeleteThread([...selectedThreadIds][0])}>Delete…</button><button type="button" onclick={clearThreadSelection}>Clear</button></div>{/if}
            <div class="side-scroll" use:panelScroll>
            <section class="sidebar-collection" aria-label="Agents group">
              <ProjectRow name="Agents" panelOpen={agentsOpen} label="Agents" icon="bot" newLabel="New agent" expanded={agentsExpanded} disabled={!!active || threadSwitching} ontoggle={() => agentsExpanded = !agentsExpanded} onactivate={() => showAgents()} onnew={() => showAgents(null,true)} />
              {#if agentsExpanded}<div class="collection-children">
            {#if agentListing.agents.length}
              <div class="agent-roster" aria-label="Saved agents">
                {#each agentListing.agents.filter(agent=>!catalogArchived('agent',agent)) as agent (agent.id)}
                  <div class="catalog-sidebar-row"><button class="side-action" aria-current={selectedAgent === agent.id ? "true" : undefined} disabled={!!active || threadSwitching} onclick={() => openAgent(agent).catch(error => { submitError = String(error) })}><AgentAvatar {agent} size={20} /><span>{agent.name}</span></button><CatalogActions name={agent.name} disabled={!!active || threadSwitching} onaction={(action,name)=>catalogAction('agent',agent,action,name)}/></div>
                {/each}
              </div>
            {/if}
            {#if creations.some(item=>item.kind==='agent'&&!agentListing.agents.some(agent=>agent.id===item.resultId))}<div class="agent-roster" aria-label="Agents in progress">{#each creations.filter(item=>item.kind==='agent'&&!catalogArchived('creation',item)&&!agentListing.agents.some(agent=>agent.id===item.resultId)) as item (item.threadId)}<div class="catalog-sidebar-row"><button class="side-action" disabled={!!active || threadSwitching} onclick={()=>openCreation(item)}><LucideIcon name="bot"/><span>{creationTitle(item, threadSummaries)}</span></button><CatalogActions name={creationTitle(item, threadSummaries)} disabled={!!active || threadSwitching} onaction={(action,name)=>catalogAction('creation',item,action,name)}/></div>{/each}</div>{/if}
              </div>{/if}
            </section>
            <section class="sidebar-collection" aria-label="Artifacts group">
              <ProjectRow name="Artifacts" panelOpen={artifactsOpen} label="Artifacts" icon="file" newLabel="New artifact" expanded={artifactsExpanded} disabled={!!active || threadSwitching} ontoggle={() => artifactsExpanded = !artifactsExpanded} onactivate={showArtifacts} onnew={() => startCreation('artifact')} />
              {#if artifactsExpanded}<div class="collection-children">
            {#if artifactChats.length}<div class="agent-roster" aria-label="Saved artifacts">{#each artifactChats.filter(item=>!catalogArchived('artifact',item)) as item (item.threadId || item.id)}<div class="catalog-sidebar-row"><button class="side-action" aria-current={item.threadId===currentThreadId?'true':undefined} disabled={!!active || threadSwitching} onclick={()=>openCreation(item)}><LucideIcon name="file-code"/><span>{item.name || item.goal}</span></button><CatalogActions name={item.name || item.goal} disabled={!!active || threadSwitching} onaction={(action,name)=>catalogAction('artifact',item,action,name)}/></div>{/each}</div>{/if}
              </div>{/if}
            </section>
            <div class="thread-search-field"><LucideIcon name="search" size={16} /><input class="thread-search" type="search" aria-label="Search threads" placeholder="Search threads" bind:value={threadSearch} oninput={filterThreads} /></div>
            <section class="project-section" aria-label="Projects">
              <ProjectRow name="Projects" label="Projects" icon="layout-dashboard" panelOpen={projectsOpen} newLabel="New project" expanded={projectsExpanded} disabled={!!active || threadSwitching || projectBusy} ontoggle={() => projectsExpanded = !projectsExpanded} onactivate={showProjects} onnew={() => { projectsExpanded = true; projectForm = 'new'; projectName = ''; projectsOpen = false }} />
              {#if projectForm}
                <form class="row-rename" onsubmit={(event) => { event.preventDefault(); void saveProject() }}>
                  <input aria-label="Project name" placeholder="Project name" maxlength="80" bind:value={projectName} disabled={projectBusy} />
                  <button type="submit" disabled={projectBusy || !projectName.trim()}>{projectForm === 'new' ? 'Create' : 'Save'}</button>
                  <button type="button" disabled={projectBusy} onclick={() => { projectForm = null }}>Cancel</button>
                </form>
              {/if}
              {#if projectsExpanded}<div class="collection-children">
              {#each projectRows as [projectId, name] (projectId)}
                <ProjectRow {name} expanded={expandedProjects.has(projectId)} disabled={!!active || threadSwitching || projectBusy}
                  ontoggle={() => selectProject(projectId)} onnew={() => newProjectThread(projectId)}
                  onrename={() => { projectForm = projectId; projectName = name }} onopen={() => openProjectFolder(projectId)} />
                {#if expandedProjects.has(projectId)}<div class="project-threads">{@render projectThreadList(projectId)}</div>{/if}
              {/each}
              {#if !projectRows.length && !projectError}<p class="side-empty">Create a project for your files.</p>{/if}
              </div>{/if}
              {#if projectError}<p class="side-empty" role="status">{projectError} <button class="quiet" onclick={() => refreshProjects()}>Retry</button></p>{/if}
            </section>
            <ThreadFilter status={includeArchived ? 'all' : archivedThreads ? 'archived' : 'active'} sort={threadSort} onchange={(status, sort) => { archivedThreads = status === 'archived'; includeArchived = status === 'all'; threadSort = sort; filterThreads() }} />
            {#each threadGroups.filter((group) => group.name === 'Pinned') as group}
              <h3 class="side-group">Pinned</h3>
              <ul class="thread-list" aria-label="Pinned threads">{#each group.threads as summary (summary.threadId)}{@render sidebarThread(summary)}{/each}</ul>
            {/each}

            {@render projectThreadList()}
            {#if !visibleThreads.length && !showFreshThread}
              <p class="side-empty">{loadingOlderThreads ? 'Searching threads…' : threadSearch.trim() ? 'No matching threads.' : archivedThreads ? 'No archived threads.' : 'No threads yet.'}</p>
            {/if}
            {#if threadListError}<p class="side-empty" role="alert">{threadListError} {#if threadListErrorAction}<button disabled={loadingOlderThreads} onclick={loadOlderThreads}>Retry</button>{/if}</p>{/if}
            {#if moreThreads && !threadListError}
              <button type="button" class="older-threads" disabled={loadingOlderThreads} onclick={loadOlderThreads}>Older threads</button>
            {/if}
            </div>
          {/if}
          {#if !sidebarCollapsed}
          <div class="side-foot">
            <div class="settings-block">
              <button class="side-action" aria-haspopup="dialog" aria-expanded={settingsOpen} aria-keyshortcuts={settingsKeyShortcut} bind:this={settingsButton} onclick={toggleSettings}><LucideIcon name="settings" size={18} /><span>Settings</span></button>
            </div>
            {#if featureFlags.cloud}
            {#if auth.name === 'signed-in'}
              <AccessPanel {tauri} subject={auth.subject} onSignOut={() => run('sign-out')} />
            {:else}
              <button class="side-action" disabled={!!active || localEntryPending} onclick={signIn}><LucideIcon name="log-in" size={18} /><span>Sign in to cloud</span></button>
            {/if}
            {/if}
          </div>
          {/if}
        </aside>
        {#if !sidebarCollapsed}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <div
            class="sidebar-divider"
            role="separator"
            aria-label="Threads"
            aria-controls="sidebar"
            aria-orientation="vertical"
            aria-valuemin={SIDEBAR_MIN_WIDTH}
            aria-valuemax={sidebarMaximum}
            aria-valuenow={sidebarWidth}
            tabindex="0"
            onpointerdown={sidebarPointerDown}
            onpointermove={sidebarPointerMove}
            onpointerup={sidebarPointerEnd}
            onpointercancel={sidebarPointerEnd}
            onkeydown={sidebarKeydown}
          ></div>
        {/if}
        <div data-panel="chat" class="thread-panel" style:--composer-height="{composerBoxHeight}px" style:--file-chip-height={changed.length ? '40px' : '0px'}>
          {#if profileAgent && !agentProfileOpen && !agentsOpen}<button type="button" data-panel-control="corner" class="quiet" aria-label="Reveal agent profile" onclick={revealAgentProfile}><LucideIcon name="panel-right-open" /></button>{/if}
        {#if draggingFiles}<div class="drop-affordance" role="status"><strong>Drop files to add them</strong><span>Saved locally · supported images sent with first prompt</span></div>{/if}
        <div class="thread-shell" data-panel-fade="chat">
        <div class="thread" role="region" aria-label={`Transcript: ${currentThreadTitle}`} bind:this={thread} onscroll={handleThreadScroll}>
          {#if historyError}<p class="history-error" role="alert">{historyError} {#if historyErrorAction}<button onclick={historyErrorAction.run}>{historyErrorAction.label}</button>{/if}</p>{/if}
          {#if messages.length === 0 && !currentCreation}<p class="empty">{auth.name === 'local' && inventoryError ? inventoryError : auth.name === 'local' && !inventory ? 'Checking available models…' : auth.name === 'local' && !chipModel ? 'Connect a model in the composer to start chatting.' : auth.name === 'local' && !currentModelAvailable(inventory) ? 'This model needs an account. Connect one or choose another.' : profileAgent ? `Chat with ${profileAgent.name} using ${modelSourceLabel}.` : 'Ask a question or request a file.'}</p>{/if}
          {#if currentCreation}
            <section class="creation-goal" aria-label="Creation goal"><strong>{currentCreation.kind === 'agent' ? 'Agent' : 'Artifact'}</strong><p>Goal: {currentCreation.goal}</p><p>Output: {currentCreation.output}</p>{#if pendingCreation}<button type="button" onclick={cancelCreation}>Cancel creation</button>{/if}</section>
            {#if !messages.length}<div class="creation-suggestions" aria-label="Creation suggestions">{#each creationOptions[currentCreation.kind] as option}<button onclick={()=>useCreationSuggestion(option)}>{option.name}</button>{/each}</div>{/if}
          {/if}
          {#each messages as message}
            {#if message.role === 'user'}
                {@const userCopyId = `user:${message.id ?? message.submissionId}`}
              <div class="user-turn">
                <div class="user-message">
                  {#if message.text}<p>{message.text}</p>{:else}<p class="missing-prompt">Prompt unavailable</p>{/if}
                  {#if message.attachments?.length}
                    <ul class="message-attachments" aria-label="Saved attachments">
                      {#each message.attachments as attachment}
                        <li><span>{attachment.displayName}</span><span>{formatByteSize(attachment.byteLength)}</span>{#if attachment.mediaType}<strong>{attachment.mediaType}</strong>{/if}</li>
                      {/each}
                    </ul>
                    <p class="attachment-delivery-rule">Supported images are sent with the first prompt.</p>
                  {/if}
                </div>
                <div class="user-message-meta message-actions">
                  {#if messageLocalTime(message.sentAt)}<time class="message-time" datetime={message.sentAt}>{messageLocalTime(message.sentAt)}</time>{/if}
                  <button type="button" aria-label={copyConfirmed(copy, userCopyId) ? 'Copied message' : 'Copy message'} data-tooltip={copyConfirmed(copy, userCopyId) ? 'Copied' : 'Copy message'} onclick={() => copyResponse({ id: userCopyId, text: message.text })}><LucideIcon name={copyConfirmed(copy, userCopyId) ? 'check' : 'copy'} variant="action" size={14} /></button>
                </div>
                {#if copyFailure(copy, userCopyId, modifierLabel)}<p class="copy-failure">{copyFailure(copy, userCopyId, modifierLabel)}</p>{/if}
              </div>
            {:else}
            <div class="response">
              {#if message.run.promptStorageNotice}
                <details class="prompt-storage-notice">
                  <summary>Prompt text stays in runtime memory for this run.</summary>
                  <p>{message.run.promptStorageNotice}</p>
                </details>
              {/if}
              {#if message.run.phase === 'acquiring-pi'}
                <p class="thinking">{runAnnouncement(message.run)}</p>
              {/if}
              {#each responseParts(message.run) as part, index}
                {#if part.type === 'actions'}
                  <ActionFeedback onopenfile={openFile} activities={part.activities} live={['thinking', 'streaming', 'pending-permission'].includes(message.run.phase)} />
                {:else if message.run.phase === 'streaming'}
                  <div class="streaming"><AssistantMarkdown {tauri} onopenlink={openChatLink} text={part.text} caret={index === responseParts(message.run).length - 1} /></div>
                {:else}<AssistantMarkdown {tauri} onopenlink={openChatLink} text={part.text} />{/if}
              {/each}
              {#if message.run.phase === 'recovering'}<p class="thinking">Restoring reply…</p>{/if}
              {#each message.run.appliedDiffs ?? [] as appliedDiff}
                <div class="applied-diff tool-card">
                  <strong>Applied file changes</strong>
                  {#if appliedDiff.diff}<CodeDiff codeDiff={appliedDiff.diff} />
                  {:else}<p>The changes were applied, but their record is no longer stored.</p>{/if}
                </div>
              {/each}
              {#if message.run.phase === 'failed'}<div class="run-error">{runFailureMessage(message.run)} <span>Retry prepares the message for review.</span> <button disabled={!message.run.prompt?.trim() || !!active || (!!draft.trim() && draft !== message.run.prompt) || !!selectedFiles.length || dictationBusy() || runtimeUpgradePending()} onclick={() => prepareRetry(message.run)}>Try again</button></div>{/if}
              {#if message.run.phase === 'cancelled'}<div class="run-error">Reply stopped. {#if message.run.prompt}<button disabled={!!active || (!!draft.trim() && draft !== message.run.prompt) || !!selectedFiles.length || dictationBusy() || runtimeUpgradePending()} onclick={() => prepareRetry(message.run)}>Try again</button>{/if}</div>{/if}
              {#if message.run.phase === 'interrupted'}<div class="run-error" role={message.run.resumeError ? 'alert' : undefined}>{message.run.resumeError ?? 'Reply interrupted.'} {#if message.run.resumable}<button disabled={!!active || dictationBusy() || runtimeUpgradePending()} onclick={() => chatController.resume(message.run)}>Resume</button>{:else if message.run.prompt}<button disabled={!!active || (!!draft.trim() && draft !== message.run.prompt) || !!selectedFiles.length || dictationBusy() || runtimeUpgradePending()} onclick={() => prepareRetry(message.run)}>Try again</button>{/if}</div>{/if}
              {#if message.run.phase === 'pending-permission' && message.run.pendingPermission}
                {@const gate = message.run.pendingPermission}
                {@const answerState = permissionState(message.run)}
                {#if gate.kind === 'editor' && gate.title === 'muniment:ask_user_question'}
                  {#key gate.gateId}<UserQuestion anchor={composerBox} payload={gate.prefill} pending={answerState?.pending ?? false} error={answerState?.error ?? ''} onanswer={(value) => answerPermission(message.run, { type: 'editor', value })} />{/key}
                {:else}
                <div class="permission-card tool-card">
                  <strong>{gate.kind === 'code_diff' ? 'Proposed file changes' : gate.title}</strong>
                  {#if gate.kind === 'confirm' && gate.message}<p>{gate.message}</p>{/if}
                  {#if gate.kind === 'code_diff' && gate.diff}<CodeDiff codeDiff={gate.diff} />
                  {:else if gate.kind === 'code_diff'}<p>Muniment will not apply a change it cannot show. Deny is the only choice.</p>{/if}
                  {#if gate.kind === 'input'}
                    <input
                      class="permission-field"
                      aria-label={gate.title}
                      placeholder={gate.placeholder ?? ''}
                      value={permissionValue(message.run)}
                      disabled={answerState?.pending}
                      oninput={(event) => setPermissionValue(message.run, event.currentTarget.value)}
                      onkeydown={(event) => permissionKeydown(event, message.run)}
                    >
                  {:else if gate.kind === 'editor'}
                    {@const hintId = `permission-editor-hint-${message.run.id}`}
                    <textarea
                      class="permission-field permission-editor"
                      aria-label={gate.title}
                      aria-describedby={hintId}
                      value={permissionValue(message.run)}
                      disabled={answerState?.pending}
                      oninput={(event) => setPermissionValue(message.run, event.currentTarget.value)}
                      onkeydown={(event) => permissionKeydown(event, message.run)}
                    ></textarea>
                    <p id={hintId} class="permission-editor-hint">{permissionGateCommitHint(gate.kind)}</p>
                  {/if}
                  <div class="permission-actions">
                    <button disabled={answerState?.pending} onclick={() => answerPermission(message.run, { type: 'cancelled' })}>Deny</button>
                    <div class="permission-approve-actions" role={gate.kind === 'select' ? 'group' : undefined} aria-label={gate.kind === 'select' ? gate.title : undefined}>
                      {#if gate.kind === 'confirm'}
                        <button disabled={answerState?.pending} onclick={() => answerPermission(message.run, { type: 'confirm', value: true })}>Allow</button>
                      {:else if gate.kind === 'select'}
                        {#each gate.options ?? [] as option}
                          <button disabled={answerState?.pending} onclick={() => answerPermission(message.run, { type: 'select', value: option })}>{option}</button>
                        {/each}
                      {:else if gate.kind === 'input' || gate.kind === 'editor'}
                        <button disabled={answerState?.pending} onclick={() => answerPermission(message.run, { type: gate.kind, value: permissionValue(message.run) })}>Submit</button>
                      {:else if gate.kind === 'code_diff' && gate.diff}
                        <button disabled={answerState?.pending} onclick={() => answerPermission(message.run, codeDiffPermissionAnswer(gate))}>Apply</button>
                      {/if}
                    </div>
                  </div>
                  {#if answerState?.error}<div class="run-error" role="alert">{answerState.error}</div>{/if}
                </div>
                {/if}
              {/if}
              {#if ['thinking', 'streaming', 'acquiring-pi', 'pending-permission', 'recovering', 'resuming'].includes(message.run.phase)}
                <div class="receipt-line live-receipt">
                  <RunMark stage={message.run.stage} />
                  <div class="response-meta">
                    <ExecutionTime startedAt={message.run.startedAt} />
                    <span class="copy-slot" aria-hidden="true"></span>
                  </div>
                </div>
              {/if}
              {#if message.run.phase === 'complete'}
                {@const summary = receiptSummary(message.run.receipt)}
                {@const rows = receiptRows({ ...message.run.receipt, routing: [] }, message.run.recalls)}
                {@const routing = message.run.receipt?.routing ?? []}
                {@const columns = receiptUsageColumns(message.run.receipt)}
                {@const hasDetails = rows.length > 0 || columns.length > 0 || routing.length > 0}
                {@const recorded = summary.route !== null || summary.model !== null || summary.time !== null || hasDetails}
                {@const expanded = hasDetails && expandedReceipts.has(message.run.id)}
                {@const failure = copyFailure(copy, message.run.id, modifierLabel)}
                <!-- One line: the receipt, then §3.2's action row at its right, copy only in this slice. -->
                <div class="receipt-line">
                  {#if recorded}
                    {#if hasDetails}
                      <button class="provenance" aria-expanded={expanded} aria-label={`${expanded ? 'Collapse' : 'Expand'} receipt: ${receiptLabel(message.run.receipt)}`} onclick={() => toggleReceipt(message.run.id)}><span class:expanded class="receipt-marker" aria-hidden="true"></span>{#if summary.route !== null}<span class="route-segment">{summary.route}</span>{/if}{#if summary.model !== null}{#if summary.route !== null}{' '}<span aria-hidden="true">→</span>{' '}{/if}<span>{summary.model}</span>{/if}</button>
                    {:else}
                      <!-- One row adds nothing beyond the line: a clock stands where the chevron would, and nothing expands. -->
                      <p class="provenance" aria-label={`Receipt: ${receiptLabel(message.run.receipt)}`}>{#if summary.route !== null}<span class="route-segment">{summary.route}</span>{/if}{#if summary.model !== null}{#if summary.route !== null}{' '}<span aria-hidden="true">→</span>{' '}{/if}<span>{summary.model}</span>{/if}</p>
                    {/if}
                  {:else}
                    <p class="provenance">Receipt unavailable</p>
                  {/if}
                  <div class="response-meta">
                    {#if summary.time !== null}<span class="receipt-time message-time"><LucideIcon name="clock" variant="action" size={12} />{summary.time}</span>{/if}
                  <div class="message-actions">
                    <button type="button" aria-label={copyLabel(copy, message.run.id)} data-tooltip={copyConfirmed(copy, message.run.id) ? 'Copied' : 'Copy message'} onclick={() => copyResponse(message.run)}>{#if copyConfirmed(copy, message.run.id)}<LucideIcon name="check" variant="action" size={14} />{:else}<LucideIcon name="copy" variant="action" size={14} />{/if}</button>
                  </div>
                  </div>
                </div>
                {#if expanded}
                  {#if columns.length}
                    <div class="receipt-usage" role="region" aria-label="Model usage" tabindex="0">
                      <table>
                        <thead><tr><td></td>{#each columns as column}<th scope="col">{column.model}</th>{/each}</tr></thead>
                        <tbody>
                          <tr><th scope="row">Cost</th>{#each columns as column}<td>{column.cost}</td>{/each}</tr>
                          <tr><th scope="row">Tokens</th>{#each columns as column}<td>{column.tokens}</td>{/each}</tr>
                        </tbody>
                      </table>
                    </div>
                  {/if}
                  <dl class="receipt-record">
                    {#each rows as row}
                      <div><dt>{row.label}</dt><dd class:route-value={row.route}>{row.value}{#each row.files ?? [] as file}<span class="recall-file">{file}</span>{/each}</dd></div>
                    {/each}
                  </dl>
                  {#if routing.length}
                    <details class="receipt-routing">
                      <summary>Routing details · {routing.length} {routing.length === 1 ? 'turn' : 'turns'}</summary>
                      {#each routing as evidence, index}
                        <section aria-label={`Routing turn ${index + 1}`}>
                          <h4>Turn {index + 1}</h4>
                          <dl class="receipt-record">
                            {#each receiptRows({ routing: [evidence] }) as row}
                              <div><dt>{row.label}</dt><dd>{row.value}</dd></div>
                            {/each}
                          </dl>
                        </section>
                      {/each}
                    </details>
                  {/if}
                {/if}
                {#if failure}<div class="run-error copy-failure">{failure}</div>{/if}
              {/if}

            </div>{/if}
          {/each}
        </div>
        {#if !pinned && hasContentBelow}<button class="latest" onclick={scrollToLatest}>↓ latest</button>{/if}
        <!-- Always mounted so the region is live before text lands in it; atomic so each
             run phase is read as one sentence, and never re-read per streamed chunk. -->
        <p class="visually-hidden" aria-live="polite" aria-atomic="true" data-testid="run-announcement">{announcement}</p>
        </div>
        <ChatComposer bind:element={composerBox}>
        {#if retryReview}
          <section aria-label="Review retry" class="retry-review">
            <p>This is a new attempt in the current conversation, including earlier replies and tool results.</p>
            <p>Earlier actions are not undone. Sending may run tools again. Review the message and reattach files if needed before sending.</p>
            {#if retryReview.chooseModel}<button type="button" onclick={() => openComposerPanel('model')}>Choose a different model for this retry</button>{/if}
            <button type="button" onclick={() => { retryReview = null; draft = ''; pickerOpen = false }}>Cancel retry</button>
          </section>
        {/if}
        {#if hasSubscriptions && capacityOpen}<Capacity {tauri} onmanage={openModelSettings} onclose={() => capacityOpen = false} />{/if}
        {#if pickerOpen}
          <ModelPicker {inventory} current={currentModel(inventory)} onchoose={chooseModel} onmanage={openModelSettings} onclose={closePicker} />
        {/if}
        {#if dictation.state === 'modelNotInstalled' && !speechInstallDismissed}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <section data-composer-panel data-panel="speech" data-panel-variant="overlay" class="speech-install-popover" aria-labelledby="speech-install-title" onkeydown={speechInstallKeydown}>
            <header class="speech-install-head">
              <strong id="speech-install-title">Speech model install</strong>
              <button type="button" class="quiet close-card" aria-label="Close speech model install" onclick={dismissSpeechInstall}><LucideIcon name="x" variant="action" size={14} /></button>
            </header>
            {#if speechInstallFacts}
              <p class="speech-install-lead">Dictation runs on this device. Voice needs one download.</p>
              <dl>
                <div><dt>Download</dt><dd>{formatBytes(speechInstallFacts.totalDownloadBytes)}</dd></div>
                <div><dt>Free disk required</dt><dd>{formatBytes(speechInstallFacts.requiredFreeBytes)}</dd></div>
              </dl>
              <details class="speech-install-details">
                <summary>Details</summary>
                <dl>
                  <div><dt>Source</dt><dd>{speechInstallFacts.sourceRepository}</dd></div>
                  <div><dt>Speech model license</dt><dd>{speechInstallFacts.speechModelLicense}</dd></div>
                  <div><dt>Voice activity model license</dt><dd>{speechInstallFacts.voiceActivityModelLicense}</dd></div>
                </dl>
              </details>
              {#if speechInstallStatus?.state === 'installing'}
                <div class="speech-install-progress">
                  <progress
                    aria-label="Speech model download progress"
                    max={Math.max(speechInstallStatus.totalBytes, 1)}
                    value={Math.min(speechInstallStatus.completedBytes, speechInstallStatus.totalBytes)}
                  ></progress>
                  <span>{formatBytes(speechInstallStatus.completedBytes)} / {formatBytes(speechInstallStatus.totalBytes)}</span>
                </div>
              {/if}
              {#if speechInstallStatus}<p role="status">{installStateWords(speechInstallStatus.state)}</p>{/if}
              {#if speechInstallStatus?.state === 'notInstalled' || speechInstallStatus?.state === 'cancelled' || speechInstallStatus?.state === 'failed'}
                <button type="button" disabled={speechInstallPending} onclick={startSpeechInstall}>Install</button>
              {:else if speechInstallStatus?.state === 'installing'}
                <button type="button" disabled={speechInstallPending} onclick={cancelSpeechInstall}>Cancel install</button>
              {/if}
            {:else if speechInstallPending}
              <p role="status">Loading install details.</p>
            {:else}
              <button type="button" onclick={openSpeechInstall}>Try again</button>
            {/if}
            {#if speechInstallError}<p class="speech-install-error" role="alert">{speechInstallError}</p>{/if}
          </section>
        {/if}
          <FileChanges files={changed} onopen={openFile} />
          {#if selectedFiles.length}
            <ul class="attachments" aria-label="Selected files">
              {#each selectedFiles as file}
                <li><button type="button" class="file-reference" onclick={() => openFile({ path: file.path })}><LucideIcon name="file-text" /><span>{file.displayName}</span></button><span>{formatByteSize(file.byteLength)}</span><button type="button" aria-label={`Remove ${file.displayName}`} onclick={() => { selectedFiles = selectedFiles.filter(({ path }) => path !== file.path) }}>Remove</button></li>
              {/each}
            </ul>
          {/if}
          <div class="composer-input">
            {#if mention}<FileMentions rootLabel={workspaceDirectory || (selectedProject ? 'Project folder' : currentThreadId ? 'Thread workspace' : 'Muniment folder')} files={mentionFiles} loading={mentionLoading} error={mentionError} selected={mentionSelected} onchoose={chooseMention} />{/if}
            <ComposerReferences commands={extensionCommandNames} text={draft} references={selectedFiles.map((file) => file.referenceName)} scrollTop={composerScrollTop} scrollLeft={composerScrollLeft} width={composerTextWidth} />
            <label class="visually-hidden" for="composer-message">Message</label>
            <textarea id="composer-message" class="reference-input" aria-autocomplete="list" aria-controls={mention ? 'file-mentions' : undefined} aria-activedescendant={mentionFiles[mentionSelected] ? `file-mention-${mentionSelected}` : undefined} onscroll={syncComposerScroll} onclick={updateMention} onkeyup={(event) => { if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) updateMention() }} onblur={() => closeMentions()} aria-describedby={threadSwitching || isDictationActive(dictation) || composerHint ? 'composer-hint' : undefined} bind:this={composer} use:focusComposerOnMount bind:value={draft} oninput={composerInput} onkeydown={keydown} rows="2" placeholder={active?.phase === 'resuming' ? 'Resuming interrupted reply…' : 'Ask anything'} disabled={composer && (active?.phase === 'resuming' || threadSwitching)}></textarea>
          </div>
          {#if draftLinks.length}<div class="composer-links" aria-label="Links in message">{#each draftLinks as url (url)}<button type="button" onclick={() => openUrl(url).catch(() => { submitError = 'This link could not be opened.' })}><LucideIcon name="globe" /><span>{url}</span></button>{/each}</div>{/if}
          {#if submitError}<p class="cancel-error" role="alert">{submitError}</p>{/if}
          {#if cancelError}<p class="cancel-error" role="alert">{cancelError}</p>{/if}
          {#if queueError}<p class="cancel-error" role="alert">{queueError}</p>{/if}
          {#if runtimeUpgradePending()}
            <section class="update-notice" aria-live="polite">
              <p class="record error-record">A Muniment update is finishing.</p>
              <p class="support">Muniment resumes on its own.</p>
            </section>
          {/if}
          <div class="composer-row" bind:this={composerRow}>
            <div class="composer-meta">
            <ComposerExtensions bind:commandNames={extensionCommandNames} bind:this={composerExtensions} bind:selection={turnExtensions} {tauri} threadId={currentThreadId} active={!!active} bind:draft onmanage={() => openSettings('extend')} />
            {#if auth.name === 'local'}
              <button type="button" class="quiet model-chip" bind:this={modelChip} aria-haspopup="dialog" aria-expanded={pickerOpen} onclick={togglePicker}>{#if chipModel}<ProviderLogo provider={inventory?.router_models?.find(entry => entry.id === chipModel.model)?.family || (chipModel.model === 'auto' ? classifierProvider(inventory?.router_classifier) : null) || chipModel.provider} size={14} />{/if}<span class="model-chip-label">{modelSourceLabel}</span><LucideIcon name={pickerOpen ? 'chevron-up' : 'chevron-right'} variant="action" size={12} /></button>
              {#if hasSubscriptions}<button type="button" class="quiet capacity-trigger" aria-haspopup="dialog" aria-expanded={capacityOpen} onclick={() => openComposerPanel(capacityOpen ? null : 'capacity')}><LucideIcon name="gauge" size={14} /><span class="model-chip-label">Capacity</span></button>{/if}
            {/if}
            {#if threadSwitching}
              <span id="composer-hint" role="status">Send waits for the thread. Your draft stays here.</span>
            {:else if isDictationActive(dictation)}
              <span id="composer-hint" class="capture-status" role="status">
                <span class="capture-meter" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i></span>
                {dictation.state === 'starting' ? 'Starting local dictation…' : 'Listening on this device…'}
              </span>
            {:else}
              {#if composerHint}<span id="composer-hint">{composerHint}</span>{/if}
            {/if}
            </div>
            <div class="composer-actions">
            {#if !active}<button type="button" class="quiet attachment-trigger" aria-label="Add files" onclick={chooseFiles}><LucideIcon name="paperclip" size={16} /></button>{/if}
              <button type="button" class="quiet composer-icon" aria-pressed={isDictationActive(dictation)} aria-keyshortcuts={ariaKeyShortcut(globalVoiceShortcutValue)} disabled={!!active || dictationFinishing} onpointerdown={voicePointerDown} onpointerup={voicePointerEnd} onpointercancel={voicePointerEnd} onkeydown={voiceKeyDown} onkeyup={voiceKeyUp} onclick={voiceClick} aria-label="Voice" aria-haspopup={dictation.state === 'modelNotInstalled' ? 'dialog' : undefined} aria-expanded={dictation.state === 'modelNotInstalled' ? !speechInstallDismissed : undefined}><LucideIcon name="mic" variant="action" size={16} /></button>
              {#if active || draft.trim()}
                <button type="button" class="composer-action" class:primary={!active} class:stop={!!active} aria-label={active ? 'Stop' : 'Send'} disabled={threadSwitching} aria-disabled={composerActionInactive() ? 'true' : undefined} onclick={composerActionClick}>
                  <LucideIcon name={active ? 'square' : 'arrow-up'} variant="action" />
                </button>
              {/if}
            </div>
          </div>
          {#if speechInstallNotice}<p class="speech-install-notice" role="status">{speechInstallNotice}</p>
          {:else if dictationError && dictation.state !== 'modelNotInstalled'}<div class="dictation-error" role="alert">{dictationError}</div>{/if}
          {#if globalVoiceError}<div class="dictation-error" role="alert">The system-wide voice shortcut is unavailable. Voice remains available from the button.</div>{/if}
        </ChatComposer>
        </div>
        {#if featureFlags.cloud && entitlementToastVisible}
          <div class="entitlement-toast" role="status">Your access changed. Some models or connections may differ.</div>
        {/if}
        {#if artifactRailOpen}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <div
            class="artifact-divider"
            role="separator"
            aria-labelledby="artifact-rail-title"
            aria-controls="artifact-rail"
            aria-orientation="vertical"
            aria-valuemin={ARTIFACT_RAIL_MIN_WIDTH}
            aria-valuemax={artifactRailMaximum}
            aria-valuenow={artifactRailWidth}
            tabindex="0"
            onpointerdown={artifactRailPointerDown}
            onpointermove={artifactRailPointerMove}
            onpointerup={artifactRailPointerEnd}
            onpointercancel={artifactRailPointerEnd}
            onkeydown={artifactRailKeydown}
          ></div>
          <aside data-panel="artifact-rail" id="artifact-rail" class="artifact-rail" aria-labelledby="artifact-rail-title">
            <header>
              <h2 id="artifact-rail-title">Artifacts</h2>
            </header>
            <div class="artifact-empty">
              <p>Ask in chat to create a document, table, or file.</p>
            </div>
          </aside>
        {/if}
        {#if featureFlags.companyRecord && recordPanelOpen}
          {#if !recordMaximized}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <div
              class="artifact-divider"
              role="separator"
              aria-labelledby="record-panel-title"
              aria-controls="record-panel"
              aria-orientation="vertical"
              aria-valuemin={railBounds('record').min}
              aria-valuemax={artifactRailMaximum}
              aria-valuenow={artifactRailWidth}
              tabindex="0"
              onpointerdown={artifactRailPointerDown}
              onpointermove={artifactRailPointerMove}
              onpointerup={artifactRailPointerEnd}
              onpointercancel={artifactRailPointerEnd}
              onkeydown={artifactRailKeydown}
            ></div>
          {/if}
          <RecordPanel {tauri} maximized={recordMaximized} refresh={recordRefresh} ontogglemaximized={toggleRecordMaximized} onask={askAboutView} />
        {/if}
        {#if filePanelOpen}
          {#if !recordMaximized}
            <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
            <div
              class="artifact-divider"
              role="separator"
              aria-labelledby="file-panel-title"
              aria-controls="file-panel"
              aria-orientation="vertical"
              aria-valuemin={railBounds('files').min}
              aria-valuemax={artifactRailMaximum}
              aria-valuenow={artifactRailWidth}
              tabindex="0"
              onpointerdown={artifactRailPointerDown}
              onpointermove={artifactRailPointerMove}
              onpointerup={artifactRailPointerEnd}
              onpointercancel={artifactRailPointerEnd}
              onkeydown={artifactRailKeydown}
            ></div>
          {/if}
          <FilePanel file={viewedFile} {tauri} threadId={currentThreadId} maximized={recordMaximized} ontogglemaximized={toggleRecordMaximized} onclose={closeRail} />
        {/if}
    {#if agentProfileOpen && profileAgent && !agentsOpen}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div class="artifact-divider agent-divider" role="separator" aria-label="Agent profile width" aria-orientation="vertical" aria-valuemin="240" aria-valuemax={agentPanelMaximum} aria-valuenow={agentPanelWidth} tabindex="0" onpointerdown={agentResize.pointerDown} onpointermove={agentResize.pointerMove} onpointerup={agentResize.pointerEnd} onpointercancel={agentResize.pointerEnd} onkeydown={agentResize.keydown}></div>
      {#key profileAgent.id}<AgentProfile earlierConversations={Object.entries(agentListing.state.threads).filter(([thread, agent]) => agent === selectedAgent && thread !== agentListing.state.primaryThreads?.[selectedAgent]).map(([threadId]) => ({ threadId, title: threadSummaries.find(item => item.threadId === threadId)?.title || "Earlier conversation" }))} history={agentListing.state.history?.[selectedAgent] || []} agent={profileAgent} {tauri} projects={projectRows} run={agentListing.state.runs[profileAgent.id]}
        onclose={() => { agentProfileOpen = false }} onchange={refreshAgents}
        ondelete={() => { agentProfileOpen = false; selectedAgent = null; void refreshAgents() }}
        onopen={(id) => chatController.openThread(id, true)} />{/key}
    {/if}
  {#if browserPanel}<div class="artifact-divider" role="separator" aria-label="Workspace panel width" aria-orientation="vertical" aria-valuemin="340" aria-valuemax={workspacePanelMaximum} aria-valuenow={workspacePanelWidth} tabindex="0" onpointerdown={workspaceResize.pointerDown} onpointermove={workspaceResize.pointerMove} onpointerup={workspaceResize.pointerEnd} onpointercancel={workspaceResize.pointerEnd} onkeydown={workspaceResize.keydown}></div>{/if}
  <WorkspacePanel context={fileContext} {requestedArtifact} {tauri} onfolder={path => { workspaceDirectory = path; if (mention) updateMention() }} navigation={requestedNavigation} onnavigationhandled={request => { if (requestedNavigation === request) requestedNavigation = null }} selected={browserPanel} onselect={showBrowser} threadId={currentThreadId} projectId={selectedProject} requestedFile={requestedWorkspaceFile} suspended={settingsOpen || !!deletingThreadId} />
  {#if agentsOpen}{#key agentsRequest}
    <AgentManager {tauri} agents={agentListing.agents} isArchived={catalogArchived} onaction={catalogAction} projects={projectRows} {threadSummaries} pending={creations.filter(item=>item.kind==='agent'&&!agentListing.agents.some(agent=>agent.id===item.resultId))} oncreate={()=>startCreation('agent')} onclose={() => { agentsOpen = false }} onselect={openAgent} onopen={openCreation} onchange={(next) => { agentListing = next }} />
  {/key}{/if}
  {#if projectsOpen}<ProjectCatalog bind:selected={catalogProject} projects={projectRows} threads={regularThreads.filter(thread => !threadOrganization[thread.threadId]?.archived)} assignments={projectCatalog.threads} busy={!!active || threadSwitching || projectBusy} error={projectError} loading={moreThreads || loadingOlderThreads} oncreate={createCatalogProject} onthread={id => threadRowClick({}, id)} onnewthread={newProjectThread} onclose={() => projectsOpen = false} />{/if}
  {#if artifactsOpen}<ArtifactCatalog items={artifactChats} isArchived={catalogArchived} onaction={catalogAction} onopen={openCreation} oncreate={()=>startCreation('artifact')} onclose={()=>artifactsOpen=false}/>{/if}
        <!-- Message actions get their own region, outside the thread shell: writing a
             copy confirmation into the run-phase region above would overwrite whatever
             a run is currently saying there, and be overwritten by the next phase. -->
        <p class="visually-hidden" aria-live="polite" aria-atomic="true" data-testid="message-action-announcement">{copyAnnouncement(copy, modifierLabel)}</p>
      </section>
    {:else if auth.name === 'error'}
      <section class="auth-state" aria-live="polite">
        <p class="record error-record">{auth.message}</p>
        <button onclick={() => run(auth.retry)}>Try again</button>
      </section>
    {/if}
    {/if}
  {/if}
</main>

  {#if deletingThreadId}
    <ConfirmDialog title={deletingThreadIds.length > 1 ? `Delete ${deletingThreadIds.length} threads?` : `Delete ${threadSummaries.find(t => t.threadId === deletingThreadId)?.title || 'this thread'}?`} cancelLabel="Cancel" confirmLabel={deletingThreadIds.length > 1 ? `Delete ${deletingThreadIds.length} threads` : 'Delete'} onDecision={approve => approve ? confirmDeleteThread(deletingThreadId) : cancelDeleteThread()}>
      <p>{deletingThreadIds.length > 1 ? 'The selected threads and their messages will be deleted.' : 'This thread and its messages will be deleted.'}</p>
    </ConfirmDialog>
  {/if}
  {#if settingsOpen}
    {#await loadSettings() then module}
    {@const Settings = module.default}
    <Settings {tauri} bind:section={settingsSection} onclose={closeSettings} homePath={onboarding.homePath} onchangehome={openHomeSettings} local={auth.name === 'local'} signInDisabled={!!active || localEntryPending} onsignin={signIn} {accountStatus} {inventory} oninventory={(next) => { inventory = next; if (accountStatus === inventoryError) accountStatus = ''; inventoryError = '' }} oncompanieschange={() => { recordRefresh += 1 }} voiceShortcut={globalVoiceShortcutValue} voiceShortcutChanging={globalVoiceChanging} onVoiceShortcutChange={changeVoiceShortcut} defaultVoiceShortcut={holdToTalkShortcut()} oncreateextension={createExtensionDraft} />
    {:catch}<p role="alert">Settings could not load. Close and reopen settings.</p>{/await}
  {/if}
{#if pairingRequests[0]}
  {#key pairingRequests[0]}
    <ConfirmDialog title="Approve Muniment connection" onDecision={decidePairing}>
      <p>The connecting program supplied these claims: kind {pairingRequests[0].claimedKind} and version {pairingRequests[0].claimedVersion}. Allow this program to access workspace {pairingRequests[0].workspace} with the scopes {pairingRequests[0].scopes.join(' and ')}?</p>
    </ConfirmDialog>
  {/key}
{/if}

<style>
  .catalog-sidebar-row {display:flex;align-items:center;min-width:0;border-radius:var(--radius-control)}
  .catalog-sidebar-row .side-action {flex:1;min-width:0}
  .catalog-sidebar-row :global(.catalog-actions) {opacity:0}
  .catalog-sidebar-row:hover :global(.catalog-actions), .catalog-sidebar-row:focus-within :global(.catalog-actions), .catalog-sidebar-row :global(.catalog-actions.opened) {opacity:1}
  @media (hover:none) { .catalog-sidebar-row :global(.catalog-actions) {opacity:1} }

  .sign-in-overlay { position: fixed; z-index: 100; top: 72px; left: 50%; transform: translateX(-50%); width: min(480px, calc(100% - 48px)); padding: 20px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--paper); box-shadow: var(--shadow-overlay); }
  main {
    min-height: 100vh;
    display: grid;
    place-content: center;
    justify-items: center;
  }

  main.onboarding-active {
    height: 100vh;
    min-height: 0;
    grid-template-rows: auto auto minmax(0, 1fr);
    grid-template-columns: minmax(0, 1fr);
    align-content: center;
    box-sizing: border-box;
    padding: 24px;
  }

  /* The full static graph surrounds the centered wordmark. */
  .lockup {
    position: relative;
    display: grid;
    place-items: center;
    width: 160px;
    height: 160px;
    color: var(--signal);
  }

  .name {
    position: absolute;
    color: var(--ink);
    font-size: var(--text-15);
    font-weight: 600;
    line-height: var(--leading-body);
    letter-spacing: -0.01em;
  }

  .meta {
    margin-top: 18px;
    font-family: var(--font-mono);
    font-size: var(--text-12);
    color: var(--muted);
  }

  .auth-state {
    margin-top: 34px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    max-width: 420px;
    text-align: center;
  }

  .auth-actions { display: flex; gap: 8px; }
  .sign-in-link { justify-self: start; color: var(--ink); font-size: var(--text-12); text-decoration: underline; }
  .primary { background: var(--ink); border-color: var(--ink); color: var(--paper); }
  .composer-actions .primary[aria-disabled="true"] { background: var(--faint); border-color: var(--border); color: var(--muted); }

  button {
    font: inherit;
    font-size: var(--text-13);
    color: var(--ink);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    padding: 5px 12px;
    cursor: pointer;
  }

  /* State is background, never a border: the composer and the rename control are the two exceptions. */
  button:hover:not(:disabled):not([aria-disabled="true"]) {
    background: var(--faint);
  }

  .primary:hover:not(:disabled):not([aria-disabled="true"]) {
    background: var(--ink);
  }

  button:disabled {
    color: var(--muted);
    cursor: default;
  }

  button[aria-disabled="true"].inactive {
    color: var(--muted);
    cursor: default;
  }

  .support {
    color: var(--muted);
  }

  .record {
    font-family: var(--font-mono);
    font-size: var(--text-12);
    color: var(--muted);
  }

  .error-record {
    line-height: var(--leading-body);
  }

  /* The band above the panels is --titlebar-band tall: the title row, then the paper gap to the panels. */
  .workspace { --frame-width: 8px; --titlebar-band: 36px; --titlebar-height: 30px; position: fixed; inset: 0; display: grid; grid-template-rows: var(--titlebar-height) minmax(0, 1fr); padding: 0 var(--frame-width) var(--frame-width); row-gap: calc(var(--titlebar-band) - var(--titlebar-height)); column-gap: var(--frame-width); background: var(--paper); }
  /* The sidebar column is the element's --sidebar-column: the kept width, or zero collapsed, and the 180ms slide carries both. */
  .workspace { grid-template-columns: minmax(0, var(--sidebar-column)) minmax(0, 1fr); grid-template-areas: "title title" "side thread"; transition: grid-template-columns 180ms ease; }
  .creation-goal { padding:12px 0; font-size:var(--text-13); color:var(--muted); } .creation-goal p { margin:4px 0; } .creation-suggestions { display:flex; flex-wrap:wrap; gap:8px; }
  .workspace.artifact-resizing, .workspace.sidebar-resizing { transition: none; }
  /* The sidebar yields frame space at the window minimum while the thread keeps 320px. */
  .agent-chat-title { display: inline-flex; align-items: center; gap: 6px; min-width: 0; max-width: 180px; }
  .agent-chat-title span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .workspace.agent-profile-open { grid-template-columns: minmax(0, var(--sidebar-column)) minmax(320px, 1fr) var(--agent-panel-width); grid-template-areas: "title title title" "side thread rail"; }
  .workspace.tools-open { grid-template-columns:minmax(0,var(--sidebar-column)) minmax(320px,1fr) var(--workspace-panel-width); grid-template-areas:"title title title" "side thread rail"; }
  @media (max-width:950px) { .workspace.tools-open { grid-template-columns:0 minmax(280px,1fr) var(--workspace-panel-width); } .workspace.tools-open .sidebar { display:none; } }
  .workspace.rail-open { grid-template-columns: minmax(0, var(--sidebar-column)) minmax(320px, 1fr) var(--artifact-rail-width); grid-template-areas: "title title title" "side thread rail"; }
  /* A maximized record takes the whole frame; the sidebar and the thread stay mounted and hidden.
     The two columns stay, so the title row's subgrid keeps its sidebar part and its thread part in place. */
  .workspace.record-maximized { grid-template-columns: minmax(0, var(--sidebar-column)) minmax(0, 1fr); grid-template-areas: "title title" "side rail"; }
  .workspace.record-maximized .thread-panel { display: none; }
  .workspace.agents-open .thread-panel { display: none; }
  .agents-side-row { position: relative; display: flex; align-items: center; }
  .agents-side-row .side-action { flex: 1; }
  .agent-add { position: absolute; right: 8px; opacity: 0; padding: 4px; line-height: 0; }
  .agents-side-row:hover .agent-add, .agents-side-row:focus-within .agent-add { opacity: 1; }
  @media (hover: none) { .agent-add { opacity: 1; } }
  .entitlement-toast { position: fixed; z-index: 4; left: 50%; bottom: 24px; max-width: calc(100% - 48px); padding: 10px 14px; transform: translateX(-50%); border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); box-shadow: var(--shadow-overlay); animation: toast-enter var(--motion-popover) var(--ease-out); }
  .drop-affordance { position: absolute; z-index: 4; inset: 0; display: grid; place-content: center; gap: 5px; background: color-mix(in srgb, var(--paper) 92%, transparent); border: 1px dashed var(--muted); border-radius: var(--radius-panel); color: var(--ink); text-align: center; pointer-events: none; }
  .drop-affordance span { color: var(--muted); font: var(--text-12) var(--font-mono); }
  /* The row ends where its 24px controls end, so the paper under them is the rest of the band. */
  .titlebar { grid-area: title; display: flex; align-items: end; gap: 8px; min-width: 0; margin: 0 calc(-1 * var(--frame-width)); padding: 0 12px; background: var(--paper); font-size: var(--text-13); user-select: none; }
  /* The title row shares the animated sidebar width so controls never cross during the panel slide. */
  @property --sidebar-column { syntax: '<length>'; inherits: true; initial-value: 195px; }
  /* The band the eye reads runs from the top edge to the panels, 36px, so its
     center is 18px. The 24px controls end at the 30px row and center there.
     Measured on macOS 26: the three 14pt lights sit at x = 9, 32 and 55 and
     AppKit tops them at 9pt, so tauri.conf.json tops them at 11pt and they
     center at 18pt too. The row starts one 9pt gap after the
     last light ends at 69pt. The sidebar toggle follows the lights. */
  .workspace.macos { --titlebar-height: 30px; --titlebar-inset: 78px; --titlebar-controls-end: 110px; transition: --sidebar-column 180ms ease; }
  .workspace.macos.artifact-resizing, .workspace.macos.sidebar-resizing { transition: none; }
  .workspace:not(.macos) .titlebar-sidebar, .workspace:not(.macos) .titlebar-thread { display: contents; }
  /* The title row is a subgrid with no margin and no padding of its own: padding
     on a subgrid shifts its tracks past the frame in WebKit, which pushed the
     artifact control off the window. The native clearance is the sidebar
     part's padding, so both parts track their panel columns in every engine. */
  .workspace.macos .titlebar { display: grid; grid-template-columns: subgrid; margin: 0; padding: 0; }
  /* The sidebar part is at least as wide as its controls, so a narrow sidebar column never hides the toggle. */
  .workspace.macos .titlebar-sidebar { grid-column: 1; position: relative; z-index: 1; box-sizing: content-box; display: flex; align-items: end; gap: 8px; min-width: calc(var(--titlebar-controls-end) - var(--titlebar-inset)); padding-left: calc(var(--titlebar-inset) - var(--frame-width)); }
  /* Artifacts sits flush right: 4px inside the 8px frame matches the 12px row padding elsewhere. */
  .workspace.macos .titlebar-thread { grid-column: 2 / -1; display: flex; align-items: end; gap: 8px; min-width: 0; padding-left: max(0px, calc(var(--titlebar-controls-end) - var(--sidebar-column) - 2 * var(--frame-width))); padding-right: 4px; }
  /* Artifacts then Record sit flush right, the update control beside them. */
  .workspace.macos .update-slot { order: 1; }
  .workspace.macos .titlebar-thread :global(.record-toggle) { order: 3; }
  .titlebar button, .titlebar input { min-width: 24px; min-height: 24px; height: 24px; padding: 0 6px; }
  .titlebar .quiet { flex: none; display: inline-flex; align-items: center; justify-content: center; gap: 6px; }
  .titlebar button:hover:not(:disabled) { background: var(--faint); border-color: transparent; }
  /* A hairline edge keeps the chip legible over the control's --faint hover. */
  .titlebar kbd { margin-left: 2px; padding: 0 4px; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--faint); }
  .update-slot { flex: 0 0 24px; height: 24px; }
  .thread-title-heading { display: flex; min-width: 24px; max-width: 100%; margin: 0; font: inherit; font-family: var(--font-heading); }
  /* The rename field keeps the title control's register while it shows. */
  /* Editing is the rename control's active state: the composer's muted hairline, no ring. */
  input.thread-title { flex: 0 1 320px; max-width: 100%; overflow: hidden; border: 1px solid var(--muted); outline: 0; background: transparent; color: var(--ink); font: inherit; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; user-select: text; }
  kbd { margin-left: 10px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .title-spacer { flex: 1; align-self: stretch; min-width: 24px; }
  .sidebar { grid-area: side; min-width: 0; display: flex; flex-direction: column; padding: 0; }
  .side-scroll { flex: 1; min-height: 0; padding: 4px 6px 8px; overflow-y: auto; }
  .project-section { margin-top: 12px; }
  .project-heading { display: flex; align-items: center; justify-content: space-between; margin: 8px 8px 2px; }
  .project-heading h3 { margin: 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .project-heading button { padding: 2px; }
  .project-threads { padding-left: 12px; }
  .side-group { margin: 10px 8px 4px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .side-group h3 { margin: 0; font: var(--text-12) var(--font-mono); }
  .side-top { padding: 8px 6px 0; }
  .side-top kbd { flex: none; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .thread-search-field { position: relative; margin-top: 4px; }
  .thread-search-field :global(.lucide) { position: absolute; left: 8px; top: 50%; transform: translateY(-50%); pointer-events: none; color: var(--muted); }
  .thread-search { width: 100%; min-width: 0; box-sizing: border-box; margin: 0; padding: 6px 8px 6px 30px; border: 0; border-radius: var(--radius-control); background: var(--faint); color: var(--ink); font: var(--text-13) var(--font-human); }
  .retry-review { font-size: var(--text-12); color: var(--muted); padding-bottom: 8px; border-bottom: 1px solid var(--border); }
  .side-empty { padding: 4px 8px; color: var(--muted); font-size: var(--text-12); }
  .side-bottom { flex: none; padding: 0 6px; }
  .side-action :global(.lucide) { flex: 0 0 18px; width: 18px; height: 18px; }
  .side-action:hover, .side-action[aria-pressed="true"], .side-action[aria-current="true"] { background: var(--faint); }
  .row-rename { display: flex; flex-wrap: wrap; gap: 4px; padding: 4px; }
  .row-rename input { width: 100%; min-width: 0; font: inherit; }
  .row-rename button { padding: 2px 6px; font-size: var(--text-12); }
  /* Clip labels during the panel slide without clipping the profile popover. */
  .older-threads, .side-action span { overflow: hidden; }
  .side-toggle { line-height: 0; }
  .side-toggle:hover:not(:disabled) { border-color: transparent; background: var(--faint); }
  .side-toggle:hover:not(:disabled) :global(.side-icon) { color: var(--ink); }
  .side-action, .thread-row { width: 100%; display: flex; align-items: center; gap: 8px; min-height: 28px; padding: 4px 8px; border-color: transparent; background: transparent; text-align: left; }
  .selection-bar { display:flex; align-items:center; gap:5px; padding:6px 10px; font-size:var(--text-12); }
  .selection-bar span { flex:1; color:var(--muted); }
  .selection-bar button { min-width:24px; min-height:24px; padding:3px 6px; font-size:var(--text-12); }
  .thread-list { padding: 0; list-style: none; }
  .older-threads { width: 100%; margin-top: 4px; border-color: transparent; background: transparent; color: var(--muted); }
  .thread-record { position: relative; }
  .thread-row { font: inherit; font-size: var(--text-13); color: var(--ink); border: 1px solid transparent; border-radius: var(--radius-control); user-select: none; -webkit-user-select: none; }
  button.thread-row:hover:not([aria-disabled="true"]) { background: var(--faint); }
  /* A selected row reads darker than the open thread's faint row, so a selection and the open thread never look alike. */
  .thread-row.selected { background: color-mix(in srgb, var(--ink) 14%, var(--surface)); }
  /* The row's delete control shows while the pointer or focus rests on the row, in the time's place. */
  .thread-actions { position: absolute; top: 50%; right: 4px; min-width: 22px; min-height: 22px; padding: 0 4px; transform: translateY(-50%); color: var(--muted); opacity: 0; pointer-events: none; }
  .thread-record:hover .thread-actions, .thread-record:focus-within .thread-actions { opacity: 1; pointer-events: auto; }
  .thread-record:hover .thread-row time, .thread-record:focus-within .thread-row time { visibility: hidden; }
  .thread-actions:hover:not(:disabled) { color: var(--ink); background: var(--faint); }
  button.thread-row[aria-disabled="true"] { opacity: .55; }
  /* The time always shows: the title takes the rest of the row and fades at its end. */
  .thread-row time { flex: none; margin-left: auto; color: var(--muted); font: var(--text-provenance) var(--font-mono); white-space: nowrap; }
  .thread-row > span { flex: 0 0 5px; }
  .thread-row-title { flex: 1 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; mask-image: linear-gradient(to right, currentColor calc(100% - 28px), transparent); }
  .thread-menu { position: fixed; z-index: 4; min-width: 88px; width: max-content; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); }
  .title-thread-actions { flex: none; }
  .thread-menu button { display: block; width: 100%; min-width: 24px; min-height: 24px; padding: 3px 8px; border-color: transparent; background: transparent; color: var(--ink); font-size: var(--text-13); text-align: left; }
  .thread-menu button:hover:not(:disabled) { border-color: transparent; background: var(--faint); }
  /* A narrow sidebar shortens the first control, never its start. */
  .side-action span { flex: 1; min-width: 0; }
  /* The foot of the sidebar: Settings above the account row, under one edge-to-edge hairline. */
  .side-foot { margin-top: auto; padding: 4px 6px 6px; border-top: 1px solid var(--border); }
  /* Settings and the account share one footer without an internal divider. */
  .settings-block { padding: 0; }
  .side-foot :global(.profile-block) { margin-top: 0; padding-top: 0; border-top: 0; }
  .side-foot :global(.profile-button) { min-height: 28px; padding: 4px 8px; }
  /* The panel is one popup over the thread, never a second settings surface. */
  /* Collapsed means gone: the column is zero wide, the empty panel drops its hairline and padding for the slide, and the thread panel takes the gap. */
  .workspace.sidebar-collapsed .sidebar { padding: 0; border-width: 0; overflow: hidden; }
  .workspace.sidebar-collapsed .thread-panel { margin-left: calc(-1 * var(--frame-width)); }
  .active-thread { background: var(--faint); }
  /* §1.2 forbids signal on selection states; the mockup's current-thread dot is ink. */
  .active-thread > span { width: 5px; height: 5px; border-radius: 50%; background: var(--ink); }
  .quiet { background: transparent; border-color: transparent; }
  /* Both dividers sit over the panel gap and draw nothing while hovered or dragged. */
  .agent-divider { grid-area: rail; }
  .artifact-divider, .sidebar-divider { z-index: 2; align-self: stretch; justify-self: start; width: var(--frame-width); margin-left: calc(-.5 * var(--frame-width)); padding: 0; border: 0; border-radius: 0; background: transparent; cursor: col-resize; touch-action: none; }
  .artifact-divider { grid-area: rail; }
  .sidebar-divider { grid-area: thread; }
  .artifact-rail { grid-area: rail; min-width: 0; padding: 22px 24px; overflow-y: auto; }
  .artifact-rail header { padding-bottom: 15px; border-bottom: 1px solid var(--border); }
  .artifact-rail h2 { margin: 3px 0 0; font-size: var(--text-17); }
  .artifact-empty { display: grid; place-items: center; align-content: center; min-height: 45%; text-align: center; }
  .artifact-empty p { margin: 0; color: var(--muted); }
  /* One grid cell: the thread fills it and the composer sits at its end, so the transcript scrolls on under the composer. */
  .thread-panel { grid-area: thread; position: relative; min-width: 0; display: grid; grid-template-rows: minmax(0, 1fr); grid-template-columns: minmax(0, 1fr); transition: margin-left 180ms ease; }
  .capacity-trigger { color: var(--muted); }
  .collection-children { padding-left: 16px; }

  .workspace.sidebar-resizing .thread-panel { transition: none; }
  /* The shell clips the transcript and its fades to the panel's rounded corners, so the fade never squares them off. */
  .thread-shell { grid-area: 1 / 1; position: relative; min-height: 0; overflow: hidden; border-radius: var(--radius-panel); }
  /* The transcript fades into the surface at both ends: a short fade under the top edge, and one above the composer that reaches the surface at the composer's midpoint, so text stays readable halfway under it. */
  /* Responses run the panel's full width inside a 36px gutter. The bottom padding is the composer and the fade, so the last line scrolls clear of both. */
  .thread { width: 100%; height: 100%; margin: 0; padding: 42px 36px calc(var(--composer-height, 120px) + var(--file-chip-height, 0px) + 64px); overflow-y: auto; }
  .latest { position: absolute; z-index: 2; left: 50%; bottom: calc(var(--composer-height, 120px) + var(--file-chip-height, 0px) + 38px); transform: translateX(-50%); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); font: var(--text-12) var(--font-mono); box-shadow: var(--shadow-overlay); }
  .empty { color: var(--muted); text-align: center; margin-top: 18vh; }
  .user-turn { margin: 0 0 28px auto; }
  .user-message { width: fit-content; max-width: 78%; margin-left: auto; padding: 9px 13px; overflow-wrap: anywhere; background: var(--faint); border-radius: var(--radius-panel); }
  .user-message-meta.message-actions { justify-content: flex-end; align-items: center; gap: 12px; margin-top: 5px; min-height: 28px; color: var(--muted); font: var(--text-provenance)/1.45 var(--font-mono); }
  .user-turn:hover .message-actions, .user-turn:focus-within .message-actions { opacity: 1; }
  .response-meta { display: flex; align-items: center; gap: 12px; margin-left: auto; color: var(--muted); font: var(--text-provenance)/1.45 var(--font-mono); flex-shrink: 0; }
  .message-actions button[data-tooltip] { position: relative; }
  .message-actions button[data-tooltip]::after { content: attr(data-tooltip); position: absolute; right: 0; bottom: calc(100% + 6px); padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); white-space: nowrap; pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  .message-actions button[data-tooltip]:hover::after, .message-actions button[data-tooltip]:focus-visible::after { opacity: 1; transition-delay: 350ms; }
  .user-message > p { margin: 0; white-space: pre-wrap; }
  .missing-prompt { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .prompt-storage-notice { margin: 0 0 8px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .prompt-storage-notice summary { min-height: 24px; line-height: 24px; cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .prompt-storage-notice summary:hover { color: var(--ink); }
  .prompt-storage-notice p { margin: 6px 0 0; overflow-wrap: anywhere; }
  .message-attachments { display: grid; justify-items: end; gap: 4px; margin: 8px 0 0; padding: 0; list-style: none; }
  .message-attachments li { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 5px 8px; max-width: 100%; padding: 5px 8px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .message-attachments strong { flex-basis: 100%; color: var(--muted); font-weight: 400; font-size: var(--text-12); }
  .attachment-delivery-rule { margin: 4px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .response { margin: 0 0 20px; }
  .response, .user-turn { content-visibility: auto; contain-intrinsic-size: auto 120px; }
  .streaming { position: relative; }
  .thinking { display: flex; align-items: center; gap: 9px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .tool-card { margin-top: 8px; padding: 8px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); font: var(--text-13) var(--font-mono); }
  .permission-card { color: var(--ink); }
  .permission-card strong, .applied-diff strong { font-weight: 600; }
  .permission-card p, .applied-diff p { margin: 4px 0 0; color: var(--muted); white-space: pre-wrap; overflow-wrap: anywhere; }
  .applied-diff { color: var(--ink); }
  .permission-field { display: block; width: 100%; margin-top: 8px; padding: 7px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); outline: 0; background: var(--surface); color: var(--ink); font: inherit; }
  .permission-field:focus { border-color: var(--muted); }
  .permission-field::placeholder { color: var(--muted); }
  .permission-editor { min-height: 84px; resize: vertical; }
  .permission-editor-hint { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .permission-actions, .permission-approve-actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .permission-actions { align-items: flex-start; margin-top: 8px; }
  .permission-approve-actions { justify-content: flex-end; margin-left: auto; }
  .permission-actions button { padding: 4px 8px; font: inherit; }
  .permission-card .run-error { margin-top: 6px; }
  /* One line under the reply: the receipt, then Copy at its right. */
  .receipt-line { display: flex; align-items: center; gap: 8px; margin-top: 10px; }
  .live-receipt { min-height: 28px; }
  .copy-slot { width: 32px; height: 28px; flex: none; }
  .provenance { display: flex; align-items: center; min-width: 24px; min-height: 24px; margin: 0; padding: 0; border: 0; background: transparent; color: var(--muted); font: var(--text-provenance)/1.45 var(--font-mono); font-variant-numeric: tabular-nums; text-align: left; overflow-wrap: anywhere; }
  /* §2.2 mono 11.5px; §1.4 records line up their figures. The shorthand resets
     font-variant-numeric, so tabular-nums follows it. */
  /* The receipt toggle takes the copy control's padding and hover, and its negative margin keeps the line's text on the prose edge. */
  button.provenance { padding: 4px 8px; margin-left: -8px; border-radius: var(--radius-control); }
  button.provenance:hover:not(:disabled) { background: var(--faint); color: var(--ink); }
  .receipt-marker { display: inline-block; width: 5px; height: 5px; border-right: 1px solid currentColor; border-bottom: 1px solid currentColor; transform: rotate(-45deg); transition: transform 120ms ease; vertical-align: 1px; }
  .receipt-marker.expanded { transform: rotate(45deg); }
  /* Segments sit apart on the row's gap, with no separator glyph between them. A flex row drops its whitespace-only text nodes, so the gap and not a space carries the spacing. */
  .provenance { gap: 9px; }
  /* §1.2 permits --signal on the route segment only. */
  .provenance .route-segment { color: var(--signal); }
  .message-time { font-family: var(--font-mono); font-size: var(--text-provenance); font-weight: 400; line-height: 1.45; font-variant-numeric: tabular-nums; }
  .receipt-time { display: inline-flex; align-items: center; gap: 4px; white-space: nowrap; }
  /* The expanded receipt sits plain under the provenance line: no box. */
  .receipt-usage { max-width: 100%; overflow-x: auto; margin-top: 8px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .receipt-usage table { border-collapse: collapse; width: auto; font: inherit; }
  .receipt-usage th, .receipt-usage td { border: 0; background: transparent; padding: 3px 24px 3px 0; text-align: left; vertical-align: top; font-weight: normal; font-variant-numeric: tabular-nums; }
  .receipt-usage th[scope="row"], .receipt-usage thead td { min-width: 100px; box-sizing: border-box; }
  .receipt-usage th[scope="col"] { white-space: nowrap; }
  .receipt-usage td { min-width: 180px; }
  .receipt-record { display: grid; row-gap: 6px; box-sizing: border-box; width: min(100%, 560px); margin: 8px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .receipt-record div { display: grid; grid-template-columns: minmax(0, 140px) minmax(0, 1fr); gap: 12px; }
  .receipt-routing { margin-top: 10px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .receipt-routing summary { cursor: pointer; }
  .receipt-routing h4 { margin: 12px 0 0; font: inherit; color: var(--ink); }
  .receipt-record dt { overflow-wrap: anywhere; }
  .receipt-record dd { margin: 0; font-family: var(--font-mono); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .receipt-record .recall-file { display: block; }
  .receipt-record .route-value { color: var(--signal); }
  /* §3.2: hover or focus reveals the row. Only opacity carries the reveal. The row
     always holds its space, so nothing reflows and nothing is ever obscured, and the
     button keeps its place in the tab order. `visibility: hidden` would strip it from
     that order exactly as `display: none` does, which would make focus unreachable
     and the :focus-within reveal below unreachable with it. */
  .message-actions { display: flex; gap: 2px; opacity: 0; transition: opacity 120ms ease; }
  .response .message-actions { opacity: 1; }
  .message-actions button { display: inline-flex; align-items: center; gap: 5px; padding: 4px 8px; border-color: transparent; background: transparent; color: var(--muted); font-size: var(--text-12); }
  .message-actions button:hover:not(:disabled) { border-color: transparent; background: var(--faint); color: var(--ink); }
  /* §1.2: focus rings are ink, never signal. */
  .message-actions button:disabled { opacity: .45; }
  @media (hover: none) { .message-actions { opacity: 1; } }
  /* Same §1.7 icon geometry as the rail, tracking whatever ink its button carries. */
  .copy-failure { margin-top: 4px; }
  .run-error { color: var(--muted); font: var(--text-12) var(--font-mono); overflow-wrap: anywhere; }
  .cancel-error, .history-error { margin: 0 0 8px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .history-error { overflow-wrap: anywhere; }
  /* The notice keeps the background service register at the composer 12px scale.
     A 15px support line would compete with the draft text. */
  .update-notice { display: grid; gap: 2px; margin: 6px 0 8px; }
  .update-notice .support { font-size: var(--text-12); }
  .run-error button { min-width: 24px; min-height: 24px; padding: 2px 6px; background: transparent; font: inherit; }
  .attachments { display: flex; flex-wrap: wrap; gap: 6px; margin: 0 -12px 8px; padding: 0 12px 8px; border-bottom: 1px solid var(--border); list-style: none; }
  .attachments li { display: flex; align-items: center; gap: 6px; max-width: 100%; padding: 4px 6px 4px 9px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .attachments span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .attachments button { padding: 1px 5px; border: 0; background: transparent; color: inherit; font-size: var(--text-12); }
  .composer-links { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .composer-links button, .attachments .file-reference { display: inline-flex; align-items: center; gap: 5px; max-width: 100%; color: var(--reference); border: 0; background: transparent; padding: 2px 0; font: inherit; cursor: pointer; }
  .composer-links button span { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  textarea.reference-input { position: relative; color: transparent; caret-color: var(--ink); }
  textarea.reference-input::placeholder { color: var(--muted); }
  @media (forced-colors: active) { textarea.reference-input { caret-color: CanvasText; } }
  .composer-input { position: relative; }
  /* No padding and no border: the composer supplies both, so the measured
     scrollHeight is pure text and the overlay lands on the same grid. */
  textarea { display: block; width: 100%; resize: none; padding: 0; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; }
  /* The input no longer keeps a spare empty row once it grows, so the action
     row carries the gap itself, matching the owner mockup's 8px .comprow rhythm. */
  .composer-row { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 8px; margin-top: 8px; color: var(--muted); font-size: var(--text-12); }
  .attachment-trigger { display: grid; place-items: center; width: 28px; height: 28px; padding: 0; }
  .composer-meta { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .composer-meta > span { flex-basis: max-content; }
  /* The three composer controls share the plus button's box: 4px padding, a 24px minimum, the control radius. */
  .model-chip { color: var(--ink); }
  .model-chip, .capacity-trigger { flex: none; display: inline-flex; align-items: center; gap: 5px; min-width: 24px; min-height: 24px; padding: 3px 4px; border: 1px solid transparent; border-radius: var(--radius-control); font: var(--text-12) var(--font-mono); white-space: nowrap; }
  .model-chip:hover:not(:disabled), .capacity-trigger:hover:not(:disabled) { background: var(--faint); }
  /* The label's line box holds the mono descenders the clip would take off a g or a p. */
  .model-chip-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; line-height: 16px; }
  .composer-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; align-items: center; gap: 6px; max-width: 100%; margin-left: auto; }
  .composer-actions button { flex-shrink: 0; white-space: nowrap; }
  .composer-icon { display: inline-flex; align-items: center; justify-content: center; min-width: 24px; min-height: 24px; padding: 4px; line-height: 0; color: var(--muted); }
  .composer-icon:hover:not(:disabled), .composer-icon[aria-pressed="true"] { color: var(--ink); background: var(--faint); }
  /* The band's one action control: an ink up arrow while the draft has text, a muted stop square in flight. */
  .composer-action { display: inline-flex; align-items: center; justify-content: center; min-width: 24px; min-height: 24px; padding: 4px; line-height: 0; }
  .composer-action.stop { background: transparent; border-color: var(--border); color: var(--muted); }
  .composer-action.stop[aria-disabled="true"] { border-color: transparent; }
  .capture-status { display: flex; align-items: center; gap: 8px; font-family: var(--font-mono); }
  .capture-meter { height: 14px; display: flex; align-items: center; gap: 2px; }
  .capture-meter i { width: 2px; height: 6px; background: var(--muted); animation: capture 900ms ease-in-out infinite alternate; }
  .capture-meter i:nth-child(2), .capture-meter i:nth-child(4) { height: 10px; animation-delay: -300ms; }
  .capture-meter i:nth-child(3) { height: 14px; animation-delay: -600ms; }
  .dictation-error { margin-top: 7px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .speech-install-popover { width: min(340px, 100%); padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); box-shadow: var(--shadow-overlay); }
  .speech-install-lead { margin: 7px 0 0; color: var(--ink); font: var(--text-13) var(--font-human); }
  .speech-install-details { margin-top: 7px; color: var(--muted); }
  .speech-install-details summary { cursor: pointer; }
  .speech-install-popover strong { font-weight: 600; }
  .speech-install-popover dl { margin: 7px 0; }
  .speech-install-popover dl div { display: grid; grid-template-columns: minmax(130px, 1fr) minmax(0, 2fr); gap: 12px; }
  .speech-install-popover dt { color: var(--muted); }
  .speech-install-popover dd { margin: 0; overflow-wrap: anywhere; }
  .speech-install-popover p { margin: 7px 0 0; color: var(--muted); }
  .speech-install-progress { display: grid; gap: 4px; margin-top: 9px; color: var(--muted); }
  .speech-install-progress progress { width: 100%; height: 6px; accent-color: var(--muted); }
  .speech-install-popover button { margin-top: 7px; padding: 4px 8px; font: inherit; }
  .speech-install-popover .speech-install-error { color: var(--oxide); }
  .speech-install-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .speech-install-popover .close-card { min-width: 24px; min-height: 24px; margin: 0; padding: 0 5px; color: var(--muted); }
  .speech-install-popover .close-card:hover { color: var(--ink); }
  .speech-install-notice { margin: 9px 0 0; padding: 0 12px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  @keyframes capture { to { transform: scaleY(.55); } }
  @keyframes toast-enter { from { opacity: 0; transform: translate(-50%, 2px); } }
  @media (prefers-reduced-motion: reduce) {
    /* Unlike the blanket duration rule, removing this animation keeps the meter
       at its full-height resting state instead of the keyframe's 55% endpoint. */
    .capture-meter i { animation: none; }
    .receipt-marker { transition: none; }
  }
</style>
