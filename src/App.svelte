<script>
  import { accountCache } from './lib/account-cache.js'
  import { migrateRecall } from './lib/memory-recall.js'
  import { startFleetPreferences } from './lib/fleet-preferences.js'
  import PopupClose from './lib/PopupClose.svelte'
  import { featureFlags } from './feature-flags.js'
  import { settingsNav, settingsPage } from './lib/settings-sections.js'
  import MachinePicker from './runtimes/MachinePicker.svelte'
  import OwnerChip from './runtimes/OwnerChip.svelte'
  import QueuedNotice from './runtimes/QueuedNotice.svelte'
  import { createFleet } from './runtimes/fleet-state.svelte.js'
  import { dedupeThreads, historyLine, offloadTargets, projectOwner, queuedFor, runMachineName, threadOwner } from './runtimes/fleet.js'
  import { activeRuntime, kindIcon, LOCAL_RUNTIME_ID, machineName, pairedRuntimes, remoteSelected, statusHint, statusLabel } from './runtimes/runtimes.js'
  import { machineStatusTone } from './lib/status-tone.js'
  import StatusChip from './lib/ui/StatusChip.svelte'
  import { floatingMenu } from './lib/floating-menu.js'
  import { panelScroll } from './lib/panel-scroll.js'
  import UserQuestion from './lib/UserQuestion.svelte'
  import { THREAD_ORGANIZATION_KEY, readThreadOrganization, listedThreads } from './lib/thread-organization.js'
  import ComposerReferences from './composer/ComposerReferences.svelte'
  import FileMentions from './composer/FileMentions.svelte'
  import { mentionQuery, insertMention, composerParts } from './composer/composer-references.js'
  import { actionGroups, responseParts } from './activity/action-feedback.js'
  import ActionFeedback from './activity/ActionFeedback.svelte'
  import ToolCall from './activity/ToolCall.svelte'
  import { ruleInput } from './activity/tool-call.js'
  import { onMount, tick, untrack } from 'svelte'
  import { workspaceOnNavigate } from './lib/workspace-panel-state.js'
  import { awaitingFrom, keptInventory, loadInventory } from './lib/inventory-loader.js'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { getCurrentWindow, UserAttentionType } from '@tauri-apps/api/window'
  import { open } from '@tauri-apps/plugin-dialog'
  import { register, unregister } from '@tauri-apps/plugin-global-shortcut'
  import { openUrl } from '@tauri-apps/plugin-opener'

  import AccessPanel from './lib/AccessPanel.svelte'
  const loadSettings = () => import('./lib/Settings.svelte')
  import ProjectCatalog from './lib/ProjectCatalog.svelte'
  import CatalogActions from './lib/CatalogActions.svelte'
  import ArtifactCatalog from './lib/ArtifactCatalog.svelte'
  import {creationStarters, creationDefaults, creationName, creationPrompt, sentCreation} from './lib/creation-prompts.js'
  import ThreadStarters from './lib/ThreadStarters.svelte'
  import { overflowLabels } from './lib/overflow-labels.js'
  import AgentManager from './lib/AgentManager.svelte'
  import WorkspacePanel from './lib/WorkspacePanel.svelte'
  import WorkspaceMenu from './lib/WorkspaceMenu.svelte'
  import AgentAvatar from './lib/AgentAvatar.svelte'
  import { agentColor } from './lib/agent-avatar.js'
  import AgentProfile from './lib/AgentProfile.svelte'
  import NotificationsPage from './notifications/NotificationsPage.svelte'
  import { isNotificationsShortcut, notificationsShortcut, notifyAllowed, readNotifications, readNotifyPreferences } from './notifications/notifications.js'
  import { agentMachine, agentRequest, agentThreads, applyRunUpdate, emptyListing, listAgents, listRuns } from './agents/agent-runtime.js'
  import { notificationKey, notificationText, shouldNotify, showNotification } from './agents/notify.js'
  import { applyApproval, emptySwarm, listSwarm, moveRequest, postTarget, swarmRoster, swarmThreads } from './agents/swarm.js'
  import { agentEnvelope, applyGroupPost, meetingSections, silentRun, timeline, visiblePosts } from './agents/transcript.js'
  import { agentMentions, insertAgentMention } from './agents/mentions.js'
  import { groupMembers } from './agents/groups.js'
  import AgentMessage from './agents/AgentMessage.svelte'
  import ThreadPost from './agents/ThreadPost.svelte'
  import AgentMentions from './agents/AgentMentions.svelte'
  import GroupCreate from './agents/GroupCreate.svelte'
  import AgentDraftChat from './agents/AgentDraftChat.svelte'
  import { requestDraft } from './agents/agent-draft.js'
  import AbsencePrompt from './agents/AbsencePrompt.svelte'
  import FolderAccessPrompt from './lib/FolderAccessPrompt.svelte'
  import { exportAgent } from './agents/import.js'
  import ApprovalList from './agents/ApprovalList.svelte'
  import AgentStatus from './agents/AgentStatus.svelte'
  import GroupMembers from './agents/GroupMembers.svelte'
  import { agentActivity, applyActivity } from './agents/activity.js'
  import { emptyCatalog, loadToolCatalog } from './agents/agent-tools.js'
  import { composerGateOptions, isPlanGate, threadApprovals } from './agents/approvals.js'
  import ChatComposer from './composer/ChatComposer.svelte'
  import ModelPicker from './lib/ModelPicker.svelte'
  import ThinkingPicker from './lib/ThinkingPicker.svelte'
  import ModePicker from './composer/ModePicker.svelte'
  import { findMode, modeIcon, nextMode, watchModes } from './agents/permission-modes.js'
  import { modelThinking, thinkingLabel, withThinkingLevel } from './lib/thinking-level.js'
  import { COMPOSER_PANEL_EVENT, openComposerPanel } from './lib/composer-panels.js'
  import UsagePage from './usage/UsagePage.svelte'
  import LimitBanner from './usage/LimitBanner.svelte'
  import SidebarActions from './lib/SidebarActions.svelte'
  import { OPEN_USAGE_EVENT, composerLimit, isUsageShortcut, machineRefs, readUsage, usageShortcut } from './usage/usage.js'
  import ComposerMenuButton from './composer/ComposerMenuButton.svelte'
  import ComposerNotice from './composer/ComposerNotice.svelte'
  import ContextMeter from './lib/ContextMeter.svelte'
  import ContextPanel from './lib/ContextPanel.svelte'
  import { compactCommand, contextMeterShown } from './lib/context-usage.js'
  import { catalogProvider, currentModel, currentModelAvailable, modelChipLabel, modelKey, providerFamily, routerHidden } from './lib/provider-catalog.js'
  import { classifierProvider } from './lib/classifier-catalog.js'
  import ProviderLogo from './lib/ProviderLogo.svelte'
  import LucideIcon from './lib/LucideIcon.svelte'
  import RowControl from './lib/RowControl.svelte'
  import AssistantMarkdown from './lib/AssistantMarkdown.svelte'
  // The file panel and the diff view pull highlight.js and diff2html, so each loads on first use.
  let filePanelModule
  const loadFilePanel = () => filePanelModule ??= import('./files/FilePanel.svelte')
  let codeDiffModule
  const loadCodeDiff = () => codeDiffModule ??= import('./lib/CodeDiff.svelte')
  import FileChanges from './files/FileChanges.svelte'
  import RunLine from './activity/RunLine.svelte'
  import { rewindOutcome, rewindUnavailable } from './activity/run-trust.js'
  import RoutingDetails from './lib/RoutingDetails.svelte'
  import { appliedDiffLabel, createChangedFiles, fileName } from './files/file-changes.js'
  import ConfirmDialog from './lib/ConfirmDialog.svelte'
  import Onboarding from './lib/Onboarding.svelte'
  import { commitType, readStoredType, stepType, typeSizeShortcutStep } from './lib/type-state.js'
  import { ARTIFACT_RAIL_MAX_WIDTH, ARTIFACT_RAIL_MIN_WIDTH, artifactRailShortcut, createRailController, defaultArtifactRailWidth, isArtifactRailShortcut, isRecordPanelShortcut, railBounds, recordPanelShortcut, shortcutDisplayLabel } from './lib/artifact-rail-state.js'
  import RecordPanel from './record/RecordPanel.svelte'
  import { bootState, errorState, registrationRetryState, statusState, waitingState } from './lib/auth-state.js'
  import { createBackgroundServiceNotice } from './lib/background-service-notice.js'
  import { createFolderAccessWatch } from './lib/folder-access.js'
  import PocketFold from './lib/PocketFold.svelte'
  import { codeDiffPermissionAnswer, composerAction, formatByteSize, messageLocalTime, permissionGateAction, permissionGateCommitHint, receiptLabel, receiptProvider, receiptRows, receiptSummary, receiptUsageColumns, promptFolders, routedTurns, runAnnouncement, runFailureDetails, runFailureMessage, settledPhases } from './lib/chat-state.js'
  import ComposerExtensions from './extend/ComposerExtensions.svelte'
  import AppUpdate from './lib/AppUpdate.svelte'
  import { createChatController } from './lib/chat-controller.js'
  import { listenForLauncher } from './lib/launcher-bridge.js'
  import { composerHeight } from './lib/composer-size.js'
  import { createDictationController } from './lib/dictation-controller.js'
  import { ariaKeyShortcut, holdToTalkShortcut, isDictationActive } from './lib/dictation-state.js'
  import { createEntitlementToast } from './lib/entitlement-toast.js'
  import { createComposerNotices } from './lib/composer-notices.js'
  import { createChatTranscriptController, transcriptKeys } from './lib/chat-transcript-controller.js'
  import { createTranscriptWindow, tailWindow, windowItems } from './lib/transcript-window.js'
  import { copyAnnouncement, copyConfirmed, copyFailure, copyLabel } from './lib/message-actions.js'
  import { onboardingLoadingState, onboardingSettingsState } from './lib/onboarding-state.js'
  import { firstRunError } from './lib/onboarding-diagnostics.js'
  import { relativeTime } from './lib/relative-time.js'
  import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH, SIDEBAR_STORAGE_KEY, createSidebarResizeController, isNewThreadShortcut, isSettingsShortcut, isSidebarShortcut, newThreadShortcut, settingsShortcut, serializeSidebarCollapsed, sidebarShortcut, storedSidebarCollapsed, storedSidebarWidth, threadRowShortcut, threadRowShortcutPosition } from './lib/sidebar-state.js'
  import { formatBytes, installStateWords } from './lib/speech-install.js'
  import { downloadsAllowed, installSpeechModel, resumeSpeechModel } from './lib/large-downloads.js'
  import RunMark from './lib/RunMark.svelte'
  import ExecutionTime from './lib/ExecutionTime.svelte'
  import SidebarSections from './sidebar/SidebarSections.svelte'
  import ProjectSettings from './sidebar/ProjectSettings.svelte'
  import CommandPalette from './sidebar/CommandPalette.svelte'
  import ProjectPicker from './sidebar/ProjectPicker.svelte'
  import { SETTINGS_PAGE, isPaletteShortcut, paletteInput, paletteShortcut, settingsPages } from './sidebar/palette.js'
  import { layerLabel, sourceLine } from './sidebar/sources.js'
  import { arrangeSidebar, emptySidebar, markRead, readMarks, runStates, shownRows, sidebarInput, statusTone } from './sidebar/sidebar-model.js'
  import { threadTitle, creationTitle } from './lib/thread-title.js'
  import { createVoiceGesture } from './lib/voice-gesture.js'
  import { createVoiceShortcutManager } from './lib/voice-shortcut.js'
  import { cleanTranscript, dictationStartOptions, readVoiceSettings } from './lib/voice-settings.js'
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
  onMount(() => { void migrateRecall(tauri); return tauri ? accountCache(tauri).start() : undefined })
  // The account snapshot follows the machine requests go to.
  $effect(() => { if (tauri) accountCache(tauri).machine(remoteRuntimeId ?? LOCAL_RUNTIME_ID) })
  onMount(() => startFleetPreferences(tauri, (...args) => window.__TAURI__?.event?.listen(...args)))
  let auth = $state(bootState)
  let localEntryError = $state('')
  let accountStatus = $state('')
  // The connected providers and their models, as Settings → Models and the picker read them.
  let inventory = $state(null)
  // The machine the inventory was read from, so Settings never seeds one
  // machine's page with another machine's rows.
  let inventoryMachine = $state(null)
  // The router leaves the models the picker hides out of the running, so a
  // routed turn answers only from a model the picker offers.
  $effect(() => {
    const hidden = routerHidden(inventory)
    if (!tauri || !inventory?.router_models?.length) return
    const cache = accountCache(tauri)
    void cache.read().then(settings => {
      if (!settings?.enabled || (settings.hidden ?? []).join('\n') === hidden.join('\n')) return
      return tauri.invoke('model_router_set_hidden', { keys: hidden }).then(next => cache.set(next))
    }).catch(() => {})
  })
  let inventoryError = $state('')
  let inventoryRequestVersion = 0
  // Inventory reads stop when the window closes.
  let inventoryClosed = false
  onMount(() => () => { inventoryClosed = true })
  let pickerOpen = $state(false)
  let thinkingOpen = $state(false)
  let modeOpen = $state(false)
  let machineOpen = $state(false)
  let contextOpen = $state(false)
  $effect(() => {
    const panel = event => {
      pickerOpen = event.detail === 'model'
      thinkingOpen = event.detail === 'thinking'
      modeOpen = event.detail === 'mode'
      machineOpen = event.detail === 'machine'
      contextOpen = event.detail === 'context'
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

  // The open thread's own model and where it comes from, from core's
  // `thread_settings` on the thread's machine. The chip shows it, and a pick
  // changes this thread alone, never the default model.
  let threadModel = $state(null)
  // The pick made before any thread exists. The first message carries it.
  let pendingModel = $state(null)
  const threadInventory = $derived.by(() => {
    const model = currentThreadId ? (threadModel?.thread_id === currentThreadId ? threadModel.model : null) : pendingModel
    return model && inventory ? { ...inventory, default_provider: model.provider, default_model: model.model } : inventory
  })
  const modelSourceLabel = $derived(modelChipLabel(threadInventory))
  const chipModel = $derived(currentModel(threadInventory))
  // The composer warns only at or near a subscription limit of the machine
  // the thread runs on, for the chip's provider when one is known, from core's
  // usage summary. It reads again every two minutes and after each reply.
  let composerLimits = $state([])
  const chipFamily = $derived(chipModel ? inventory?.router_models?.find(entry => entry.id === chipModel.model)?.family || (chipModel.model === 'auto' ? null : providerFamily(chipModel.provider)) : null)
  const composerWarning = $derived(composerLimit(composerLimits, chipFamily))
  async function refreshComposerLimits() {
    if (!tauri || !(auth.name === 'local' || auth.name === 'signed-in')) { composerLimits = []; return }
    const machine = remoteRuntime ? { id: remoteRuntime.id, name: remoteRuntime.name } : { id: LOCAL_RUNTIME_ID, name: 'This computer' }
    try { composerLimits = (await readUsage(tauri, [machine], { range: '24h' }))?.pools ?? [] } catch (_) { composerLimits = [] }
  }
  onMount(() => {
    const poll = () => { if (!document.hidden) void refreshComposerLimits() }
    const timer = setInterval(poll, 120_000)
    return () => clearInterval(timer)
  })
  // The levels the chip's model thinks at and the one a send carries. A model
  // without reasoning shows no thinking control and sends no level.
  const chipThinking = $derived(auth.name === 'local' ? modelThinking(inventory, chipModel) : null)
  let thinkingChip = $state()

  let settingsOpen = $state(false)
  let groupCreateOpen = $state(false)
  let agentsOpen = $state(false)
  // The page that takes the thread's place from the sidebar's foot: 'usage', 'notifications' or none.
  let fullPage = $state(null)
  let artifactsOpen = $state(false)
  let projectsOpen = $state(false)
  // The project whose settings take the thread's place, from the sidebar's Projects list.
  let settingsProject = $state(null)
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
  // An agent or artifact whose thread holds the running reply keeps its actions until that reply settles.
  function catalogHoldsRun(kind, item) {
    if (switchLocked) return true
    if (!runningThreadId) return false
    const threadIds = kind === 'agent'
      ? [...Object.keys(agentThreadMap).filter(id => agentThreadMap[id] === item.id), ...creations.filter(plan => plan.resultId === item.id).map(plan => plan.threadId)]
      : [item.threadId, ...creations.filter(plan => plan.threadId === item.threadId).map(plan => plan.threadId)]
    return threadIds.includes(runningThreadId)
  }
  async function catalogAction(kind, item, action, name) {
    if (threadSwitching || catalogHoldsRun(kind, item)) throw new Error('Finish the current reply before changing this item.')
    const plans = kind === 'agent' ? creations.filter(plan=>plan.resultId===item.id) : creations.filter(plan=>plan.threadId===item.threadId)
    if (action === 'archive' || action === 'restore') {
      // An archived agent stops starting itself: its routines turn off.
      if (kind === 'agent' && action === 'archive' && (item.routines ?? []).some(routine => routine.enabled)) {
        const { agent: latest } = await agentRequest(tauri, item.runtimeId, 'agent.read', { id: item.id })
        await agentRequest(tauri, item.runtimeId, 'agent.save', { agent: { ...latest, schedule: null, routines: latest.routines.map(routine => ({ ...routine, enabled: false })) } })
        await refreshAgents()
      }
      const next = {...catalogArchive,[catalogKey(kind,item)]:action==='archive'}
      localStorage.setItem(CATALOG_ARCHIVE_KEY,JSON.stringify(next))
      catalogArchive = next
      return
    }
    if (action === 'rename') {
      if (kind === 'agent') {
        const { agent: latest } = await agentRequest(tauri, item.runtimeId, 'agent.read', { id: item.id })
        await agentRequest(tauri, item.runtimeId, 'agent.save', { agent: { ...latest, schedule: null, name } })
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
      const threadIds = kind === 'agent' ? Object.keys(agentThreadMap).filter(id=>agentThreadMap[id]===item.id) : [item.threadId].filter(Boolean)
      const next = {...threadOrganization}
      for (const id of [...threadIds,...plans.map(plan=>plan.threadId)]) next[id] = {...next[id],archived:true}
      localStorage.setItem(THREAD_ORGANIZATION_KEY,JSON.stringify(next))
      threadOrganization = next
      if (kind === 'agent') {
        await agentRequest(tauri, item.runtimeId, 'agent.delete', { id: item.id, remove_memory: false })
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
  async function startCreation(kind, chosen = creationDefaults(kind), text = '') {
    if (remoteSelected(runtimeList)) await onThisComputer()
    if (switchLocked) return
    if (!await newSidebarThread(selectedProject) || !currentThreadId) return
    pendingCreation = { threadId: currentThreadId, kind, goal: chosen.goal, output: chosen.output, resultId: null }
    draft = text
    await tick()
    composer?.focus()
  }
  // A new agent is a chat on this computer: each message runs core's
  // `agent.draft`, and the reply is the agent's draft. The chat's creation
  // record keeps the messages and drafts, so the chat stays to revise the
  // agent. New agent, the agent starters, a project's Add a trigger and a
  // thread's Make this an agent open one. A starter's description, or the
  // thread the agent takes over, is its first message.
  let newAgentChat = $state(null)
  let agentChatPending = $state(null)
  let agentChatError = $state('')
  const agentChat = $derived.by(() => {
    if (!currentThreadId) return null
    const record = creations.find((item) => item.threadId === currentThreadId && item.kind === 'agent' && item.turns?.length)
    if (record) return { threadId: currentThreadId, record, fromThread: null }
    return newAgentChat?.threadId === currentThreadId ? { ...newAgentChat, record: null } : null
  })
  // An agent chat's title: its saved agent's name, else New agent.
  function agentChatTitle(record) {
    return roster.find((agent) => agent.id === record?.resultId)?.name || 'New agent'
  }
  const agentChatRows = $derived(creations.filter((item) => item.kind === 'agent' && item.turns?.length).map((item) => ({ threadId: item.threadId, title: agentChatTitle(item), updatedAt: item.turns.at(-1)?.at ?? '', agentId: item.resultId ?? null })))
  // The model an agent chat's agent runs: its own, else the default.
  const agentChatModel = $derived.by(() => {
    const own = roster.find((agent) => agent.id === agentChat?.record?.resultId)?.model
    if (own?.provider && own?.id && inventory) return modelChipLabel({ ...inventory, default_provider: own.provider, default_model: own.id })
    const fallback = inventory ? modelChipLabel(inventory) : ''
    return fallback ? `${fallback} · default` : ''
  })
  async function openAgentChat({ description = '', fromThread = null, projectId = null } = {}) {
    if (remoteSelected(runtimeList)) await onThisComputer()
    if (switchLocked) return
    if (settingsOpen) closeSettings()
    projectsOpen = false; artifactsOpen = false; agentsOpen = false; agentProfileOpen = false; closeRail()
    if (!await newSidebarThread(projectId) || !currentThreadId) return
    newAgentChat = { threadId: currentThreadId, fromThread }
    agentChatError = ''
    draft = ''
    if (fromThread) await sendAgentChat(`Make an agent from ${fromThread.title || 'this thread'}`)
    else if (description.trim()) await sendAgentChat(description)
    else { await tick(); composer?.focus() }
  }
  // Sends one message to the open agent chat. A failed draft leaves the
  // message in the composer to send again.
  async function sendAgentChat(text) {
    const chat = agentChat
    const message = String(text ?? '').trim()
    if (!chat || !message || agentChatPending) return
    agentChatPending = { threadId: chat.threadId, message }
    agentChatError = ''
    try {
      const answer = await requestDraft(tauri, LOCAL_RUNTIME_ID, { chatId: chat.threadId, message, threadId: chat.record ? null : chat.fromThread?.id ?? null })
      if (answer?.creation) {
        creationsRevision += 1
        creations = [...creations.filter((item) => item.threadId !== answer.creation.threadId), answer.creation]
      }
    } catch (error) {
      if (agentChat?.threadId === chat.threadId) {
        agentChatError = String(error?.message ?? error)
        if (!draft.trim()) draft = message
      }
    } finally { agentChatPending = null }
  }
  function sendComposerToAgentChat() {
    const text = draft
    draft = ''
    void sendAgentChat(text)
  }
  // A saved agent's chat stays its creation chat, and the agent shows in the lists.
  async function agentChatSaved(agent, threadId) {
    const record = creations.find((item) => item.threadId === threadId)
    if (record && record.resultId !== agent.id) {
      const plan = await tauri.invoke('creation_save', { creation: { ...$state.snapshot(record), resultId: agent.id } }).catch(() => null)
      if (plan) { creationsRevision += 1; creations = [...creations.filter((item) => item.threadId !== threadId), plan] }
    }
    await refreshAgents()
  }
  async function openBuiltAgent(agent) {
    await refreshAgents()
    const entry = roster.find((item) => item.id === agent.id && (item.runtimeId ?? 'local') === (agent.runtimeId ?? 'local')) ?? roster.find((item) => item.id === agent.id)
    if (entry) await openAgent(entry).catch((error) => { submitError = String(error) })
  }
  // A new thread's starter card: a creation chat, or a prompt in the composer,
  // after the file picker for a starter about files.
  async function pickThreadStarter(starter) {
    if (starter.kind === 'artifact') return startCreation('artifact', { goal: starter.goal, output: starter.output })
    if (starter.kind === 'agent') return openAgentChat()
    if (starter.kind === 'files') {
      await chooseFiles()
      if (!selectedFiles.length) return
    }
    draft = starter.prompt
    await tick()
    composer?.focus()
    composer?.setSelectionRange?.(draft.length, draft.length)
  }
  async function useCreationSuggestion(chosen) {
    if (pendingCreation) {
      pendingCreation = { ...pendingCreation, goal: chosen.goal, output: chosen.output }
    }
    await tick()
    composer?.focus()
  }
  function cancelCreation() {
    pendingCreation = null
    draft = ''
    selectedFiles = []
    turnExtensions = { enabled: [], blocked: [], automatic: null }
    composer?.focus()
  }
  // Agents, artifacts and projects live on this computer, so opening one
  // sends requests there again.
  async function onThisComputer() {
    if (remoteSelected(runtimeList)) await selectRuntime(runtimeList.runtimes.find((runtime) => runtime.local))
  }

  // A new thread starts on a machine that answers: one picked earlier that is
  // now offline or revoked this device gives way to this computer.
  $effect(() => {
    if (!freshThread || !remoteSelected(runtimeList) || active) return
    const picked = runtimeList.runtimes.find((runtime) => runtime.id === runtimeList.selected)
    if (picked && statusHint(picked)) void onThisComputer()
  })

  async function openCreation(item) {
    if (remoteSelected(runtimeList)) await onThisComputer()
    if (switchLocked) return
    if (!item.threadId) {
      if (!await newSidebarThread(selectedProject) || !currentThreadId) return
      try {
        const plan=await tauri.invoke('creation_save',{creation:{threadId:currentThreadId,kind:'artifact',goal:`Maintain ${item.name}`,output:'The saved artifact and its revisions',resultId:item.id}})
        creationsRevision += 1
        creations=[...creations,plan]
      } catch(e) {submitError=String(e);return}
    } else if (!await chatController.openThread(item.threadId)) return
    projectsOpen=false; agentsOpen=false; fullPage = null; artifactsOpen=false; agentProfileOpen=false
    const artifactId=item.resultId || item.id
    if (artifactId && item.kind !== 'agent') {requestedArtifact=artifactId;showBrowser('artifacts')}
  }
  // The first time a creation chat saves its artifact, the preview opens beside it.
  async function revealSavedArtifact() {
    const threadId = currentThreadId
    const before = creations.find(item => item.threadId === threadId)
    if (!threadId || (before && before.kind !== 'artifact')) return
    await refreshCreations()
    const plan = creations.find(item => item.threadId === threadId)
    if (threadId !== currentThreadId || plan?.kind !== 'artifact' || !plan.resultId || plan.resultId === before?.resultId) return
    requestedArtifact = plan.resultId
    showBrowser('artifacts')
  }
  async function showArtifacts() {
    if (remoteSelected(runtimeList)) await onThisComputer()
    await refreshCreations()
    artifactsOpen=true;projectsOpen=false; agentsOpen=false; fullPage = null;agentProfileOpen=false;closeRail()
  }
  let browserPanel = $state(null)
  // The tool put aside while Settings or a catalog page shows.
  let parkedPanel = null
  const threadView = $derived(!settingsOpen && !agentsOpen && !artifactsOpen && !projectsOpen && !fullPage)
  $effect(() => {
    const view = threadView
    untrack(() => { ({ panel: browserPanel, parked: parkedPanel } = workspaceOnNavigate({ panel: browserPanel, parked: parkedPanel }, view)) })
  })
  let workspacePanelWidth = $state(500)
  let workspacePanelMaximum = $state(1200)
  let workspacePanelPointer = $state()

  let toolsMenuOpen = $state(false)
  let requestedNavigation = $state(null)
  function openChatLink(url) { requestedNavigation = {url}; showBrowser('browser') }
  let requestedWorkspaceFile = $state(null)
  function showBrowser(mode) { if (mode === 'artifacts') { artifactsOpen = false; projectsOpen = false; agentsOpen = false; fullPage = null }; browserPanel = mode; agentProfileOpen = false; closeRail(); workspaceResize.fit() }
  let agentsRequest = $state(0)
  let agentPanelId = $state(null)
  let agentCreateNew = $state(false)
  async function showAgents(id = null, create = false) {
    // A new agent starts in a creation chat on this computer.
    if (create && remoteSelected(runtimeList)) await onThisComputer()
    await Promise.all([refreshAgents(),refreshCreations()])
    if (create) return openAgentChat()
    projectsOpen=false; artifactsOpen=false; fullPage = null; agentsOpen=true; agentProfileOpen=false; agentsRequest+=1;closeRail()
  }
  let agentProfileOpen = $state(false)
  let agentPanelWidth = $state(320)
  let agentPanelMaximum = $state(560)
  let agentPanelPointer = $state()
  let lastProfileAgent = null
  // The thread a run's link opened. Its agent's profile stays closed, so
  // the thread keeps the view until the user reveals the profile.
  let profileHeldFor = null
  function revealAgentProfile() { browserPanel = null; projectsOpen = false; agentsOpen = false; fullPage = null; closeRail(); agentProfileOpen = true; agentResize.fit() }
  $effect(() => {
    const id = profileAgent?.id
    if (id && id !== lastProfileAgent && !browserPanel && untrack(() => profileHeldFor !== currentThreadId)) untrack(() => revealAgentProfile())
    lastProfileAgent = id
  })
  let agentOpening = $state(false)
  let selectedAgent = $state(null)
  // The agent whose thread is open, and the agent whose profile shows.
  const threadAgent = $derived(roster.find(agent => agent.id === selectedAgent))
  const profileAgent = $derived(roster.find(agent => agent.id === (profileOnlyAgent ?? selectedAgent)))
  // Agents from every machine the app reaches, each with its `runtimeId`.
  let agentListing = $state(emptyListing())
  // The swarm every reachable machine reports: members, the agent directory,
  // groups, pending approvals and the absence pause.
  let swarm = $state(emptySwarm())
  // This computer's fleet: its sync status, the actions it forwarded to an
  // offline owner and its model accounts.
  const fleet = createFleet(tauri)
  // Every agent the sidebar and catalog list: the listed ones, then the ones
  // only the swarm directory names, each with its primary machine.
  // Each carries its activity: the newest `agent.activity` event, else what
  // its machine or the swarm directory listed.
  let liveActivity = $state({})
  const roster = $derived(swarmRoster(agentListing, swarm, runtimeList).map(agent => ({ ...agent, activity: agentActivity(agent, liveActivity, swarm.agents) })))
  // Which agent each known thread belongs to, replicated threads included.
  const agentThreadMap = $derived({ ...swarmThreads(swarm), ...agentThreads(agentListing) })
  // The group whose thread is open, and its members.
  const threadGroup = $derived(swarm.groups.find(group => group.thread_id === currentThreadId) ?? null)
  const threadGroupMembers = $derived(groupMembers(threadGroup, roster))
  let swarmVersion = 0
  async function refreshSwarm() {
    const version = ++swarmVersion
    try {
      const next = await listSwarm(tauri, runtimeList)
      if (version === swarmVersion) swarm = next
    } catch (_) { /* The last swarm stays until a machine answers. */ }
  }
  // An agent on a paired machine whose conversation has not started yet
  // opens its profile alone.
  let profileOnlyAgent = $state(null)
  // A listing that a later request overtook is dropped, so a slow answer
  // cannot unselect an agent whose thread just started.
  let agentListVersion = 0
  async function refreshAgents() {
    void refreshSwarm()
    const version = ++agentListVersion
    try {
      const value = await listAgents(tauri, runtimeList)
      if (version !== agentListVersion) return
      agentListing = value
      selectedAgent = { ...swarmThreads(swarm), ...agentThreads(value) }[currentThreadId] ?? startingAgent ?? null
    }
    catch (_) { /* The manager presents errors and retry state. */ }
  }
  const showAgentMachines = $derived(pairedRuntimes(runtimeList).length > 0 || swarm.members.length > 1)
  // A group's thread opens on the machine that holds it, like any thread.
  async function openGroup(group) {
    if (switchLocked) return
    if (!threadSummaries.some(thread => thread.threadId === group.thread_id)) await chatController.refreshThreads()
    if (group.thread_id !== currentThreadId && !await chatController.openThread(group.thread_id)) return
    selectedAgent = null; profileOnlyAgent = null; agentProfileOpen = false
    projectsOpen = false; agentsOpen = false; fullPage = null; artifactsOpen = false
  }
  async function openAgent(agent) {
    if (switchLocked || agentOpening) return
    agentOpening = true
    try {
      // An agent's conversation lives on its machine, and the thread opens there.
      const machine = agentMachine(runtimeList, agent)
      const mapping = agentThreadMap
      const threadId = agentListing.primaryThreads?.[agent.id] ?? (agent.replica && threadSummaries.some(thread => thread.threadId === agent.primaryThread) ? agent.primaryThread : null) ?? (mapping[currentThreadId] === agent.id ? currentThreadId
        : threadSummaries.find(thread => mapping[thread.threadId] === agent.id && !threadOrganization[thread.threadId]?.archived)?.threadId)
      profileOnlyAgent = null
      if (threadId) {
        if (threadId !== currentThreadId && !await chatController.openThread(threadId, true)) return
      } else if (!machine.local || agent.replica) {
        // Its first run starts the conversation on its machine.
        profileOnlyAgent = agent.id
        projectsOpen = false; agentsOpen = false; fullPage = null; artifactsOpen = false
        if (!browserPanel) revealAgentProfile()
        return
      } else await startAgentThread(agent)
      selectedAgent = agent.id
      selectedProject = agent.projectId || null
      projectsOpen = false; agentsOpen = false; fullPage = null; artifactsOpen = false
      if (!browserPanel) revealAgentProfile()
    } finally { agentOpening = false }
  }
  // An agent's profile alone, beside whatever thread is open.
  function openAgentProfile(agent) {
    profileOnlyAgent = agent.id
    projectsOpen = false; agentsOpen = false; fullPage = null; artifactsOpen = false
    if (!browserPanel) revealAgentProfile()
  }
  // The agent whose thread is starting keeps its profile open until the
  // listing names that thread.
  let startingAgent = null
  async function startAgentThread(agent) {
    selectedAgent = agent.id
    selectedProject = agent.projectId || null
    startingAgent = agent.id
    try {
      if (!await chatController.newThread()) { selectedAgent = agentThreadMap[currentThreadId] ?? null; throw new Error('The agent thread could not be started.') }
      threadSearch = ''
      filterThreads()
      await refreshAgents()
    } finally { startingAgent = null }
  }
  let settingsSection = $state('models')
  // The words in the Settings search, which narrow its sidebar.
  let settingsQuery = $state('')
  const isMacPlatform = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform ?? '')
  // The runtimes the app can attach to. While a paired runtime is selected,
  // threads, models and settings come from it, and this computer's agents,
  // artifacts and projects step aside.
  let runtimeList = $state(null)
  const remoteRuntime = $derived(remoteSelected(runtimeList) ? activeRuntime(runtimeList) : null)
  // Each runtimes-changed event brings a new list object, so the reads that
  // follow the selected machine depend on its id alone and a name or state
  // change starts none of them.
  const remoteRuntimeId = $derived(remoteRuntime?.id ?? null)
  const REMOTE_FILES_NOTE = 'Files stay on this computer. A paired runtime cannot read them.'
  // A first-run choice the providers page opens at once.
  let settingsConnectProvider = $state(null)
  let settingsButton = $state()
  // The control that opened Settings takes focus back when it closes.
  let settingsOpener = null

  function openSettings(section = 'models', opener = settingsButton) {
    settingsSection = section
    settingsOpener = opener ?? null
    pickerOpen = false
    settingsOpen = true
    void tick().then(() => document.querySelector('#sidebar nav [aria-current="true"]')?.focus())
  }

  async function createExtensionDraft(kind) {
    if (switchLocked) return
    if (!await newSidebarThread(selectedProject)) return
    closeSettings()
    draft = `Help me create a ${kind}. Ask about its purpose, then save it in managed extension storage.`
    await tick()
    composer?.focus()
  }

  // Which paired machines answer, so the thread list follows a machine that
  // goes offline, comes back or revokes this device.
  const machineStates = (list) => (list?.runtimes ?? []).filter((runtime) => !runtime.local).map((runtime) => `${runtime.id}:${runtime.status}`).join(' ')

  function applyRuntimes(next) {
    if (!next?.runtimes) return
    const previous = runtimeList?.selected
    const previousStates = runtimeList ? machineStates(runtimeList) : null
    runtimeList = next
    if (previous && previous !== next.selected) void runtimeSwitched()
    if (previousStates !== null && previousStates !== machineStates(next)) void chatController?.refreshThreads()
    // Agents live on every machine that answers, so the list follows them too.
    if (previousStates !== machineStates(next)) void refreshAgents()
  }

  // The thread list holds every machine's threads, so a change of the machine
  // requests go to only loads that machine's models.
  async function runtimeSwitched() {
    await refreshInventory()
  }

  async function selectRuntime(runtime) {
    try {
      applyRuntimes(await tauri.invoke('runtime_select', { id: runtime.id }))
    } catch (error) {
      submitError = String(error?.message ?? error)
    }
  }

  onMount(() => {
    if (!tauri) return
    let alive = true
    let stop
    Promise.resolve(tauri.invoke('runtime_list')).then(applyRuntimes).catch(() => {})
    Promise.resolve(window.__TAURI__?.event?.listen('runtimes-changed', ({ payload }) => applyRuntimes(payload)))
      .then((unlisten) => { if (alive) stop = unlisten; else unlisten?.() })
      .catch(() => {})
    return () => { alive = false; stop?.() }
  })

  function closeSettings() {
    settingsOpen = false
    settingsConnectProvider = null
    // The sidebar's Settings control remounts when Settings closes, so a detached opener hands focus to it.
    void tick().then(() => {
      const opener = settingsOpener?.isConnected ? settingsOpener : settingsButton ?? composer
      opener?.focus?.()
    })
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
    if (!pickerOpen && needsModel()) openModelProviders()
    else if (pickerOpen) closePicker()
    else openComposerPanel('model')
  }

  // A picker row moves its model between the current list and Legacy models.
  // A hidden model also leaves the router's running, and hiding the saved
  // default moves the runtime's choice to the row the picker now leads with.
  async function setModelShown(model, shown) {
    const key = modelKey(model.provider, model.choice)
    try {
      await tauri.invoke('local_mode_set_model_hidden', { provider: model.provider, model: model.choice, hidden: !shown })
      const next = { ...inventory, hidden: (inventory?.hidden ?? []).filter(entry => entry !== key), shown: (inventory?.shown ?? []).filter(entry => entry !== key) }
      next[shown ? 'shown' : 'hidden'].push(key)
      inventory = next
      if (!shown && modelKey(next.default_provider, next.default_model) === key) {
        const fallback = currentModel(next)
        if (fallback) {
          await tauri.invoke('local_mode_set_default_model', { provider: fallback.provider, model: fallback.model })
          inventory = { ...inventory, default_provider: fallback.provider, default_model: fallback.model }
        }
      }
    } catch (_) {
      accountStatus = 'Muniment could not save the model list. Try again.'
    }
  }

  function closeThinking() {
    thinkingOpen = false
    void tick().then(() => thinkingChip?.focus())
  }

  // The runtime keeps one level per model, so every window and the next
  // send see the choice.
  async function chooseThinkingLevel(level) {
    const model = chipModel
    closeThinking()
    if (!model || level === chipThinking?.level) return
    const before = chipThinking?.level
    inventory = withThinkingLevel(inventory, model.provider, model.model, level)
    try {
      await tauri.invoke('local_mode_set_thinking_level', { provider: model.provider, model: model.model, level })
    } catch (_) {
      inventory = withThinkingLevel(inventory, model.provider, model.model, before)
      accountStatus = 'Muniment could not save the thinking level. Try again.'
    }
  }

  let threadModelRequest = 0
  async function refreshThreadModel() {
    const request = ++threadModelRequest
    const threadId = currentThreadId
    if (!tauri || !threadId || auth.name !== 'local') { threadModel = null; return }
    try {
      const answer = await tauri.invoke('thread_settings', { threadId })
      if (request === threadModelRequest) threadModel = answer?.thread_id === threadId ? answer : null
    } catch (_) { if (request === threadModelRequest) threadModel = null }
  }
  // The thread's model reads again when the thread or the default changes,
  // and once its first message gives a new thread an id.
  $effect(() => {
    void currentThreadId; void inventory?.default_provider; void inventory?.default_model; void remoteRuntimeId; void auth.name
    untrack(() => {
      if (currentThreadId) pendingModel = null
      void refreshThreadModel()
    })
  })
  // The thread's project, for the model picker's project actions. An
  // agent's thread, a group's and a thread in no project take none.
  const modelProject = $derived(!currentThreadId || agentThreadMap[currentThreadId] || threadGroup ? null : (projectCatalog.threads[currentThreadId] ?? null))
  const modelLayerNames = $derived({ projectName: projectCatalog.projects[threadModel?.project_id] ?? '', agentName: threadAgent?.name ?? '' })
  const modelSource = $derived(!currentThreadId ? (pendingModel ? 'this thread' : 'default') : layerLabel(threadModel?.layer ?? 'default', modelLayerNames))
  const modelActions = $derived.by(() => {
    if (!currentThreadId || !threadModel) return []
    const actions = []
    if (threadModel.layer === 'thread') actions.push({ id: 'reset', label: threadModel.seed?.model ? 'Use the project model' : 'Use the default model' })
    if (modelProject && threadModel.layer !== 'project' && threadModel.model) actions.push({ id: 'project-default', label: 'Make project default' })
    // The machine's default changes only on this explicit choice.
    const model = threadModel.model
    if (model && inventory && (inventory.default_provider !== model.provider || inventory.default_model !== model.model)) actions.push({ id: 'default', label: 'Make default' })
    return actions
  })

  // The newest turn's run line names where its model and mode come from
  // while the thread still runs on them.
  const newestRunId = $derived(messages.findLast((message) => message.run && settledPhases.has(message.run.phase))?.run.id ?? null)
  function runSources(run) {
    const receipt = run?.receipt ?? {}
    const model = typeof receipt.model === 'string' ? receipt.model : ''
    const sameModel = !!chipModel && (model === chipModel.model || model.endsWith(`/${chipModel.model}`))
    return {
      model: sameModel && threadModel?.thread_id === currentThreadId ? modelSource : null,
      mode: receipt.permission_mode && receipt.permission_mode === composerMode?.id && composerThreadId ? modeSource : null,
    }
  }

  // A pick changes the open thread's model. With no thread yet it waits for
  // the first message, which starts the thread with it.
  async function chooseModel(provider, model) {
    if (modelChoicePending) return
    closePicker()
    const threadId = currentThreadId
    const reviewed = () => { if (retryReview) retryReview = { ...retryReview, chooseModel: retryReview.chooseModel && retryReview.previousProvider === provider && retryReview.previousModel === model } }
    if (!threadId) { pendingModel = { provider, model }; reviewed(); return }
    modelChoicePending = true
    const before = threadModel
    threadModel = { ...(before ?? {}), thread_id: threadId, model: { provider, model }, layer: 'thread' }
    try {
      const answer = await tauri.invoke('thread_settings_set', { threadId, model: { provider, model } })
      if (currentThreadId === threadId) threadModel = answer
      reviewed()
    } catch (_) {
      if (currentThreadId === threadId) threadModel = before
      accountStatus = 'Muniment could not save the model choice. Try again.'
    } finally { modelChoicePending = false }
  }

  // The picker's foot: the open thread back on the model it started with,
  // or its model as its project's default for new threads.
  async function runModelAction(id) {
    const threadId = currentThreadId
    const model = threadModel?.model
    if (!threadId || modelChoicePending) return
    closePicker()
    modelChoicePending = true
    try {
      if (id === 'reset') {
        const answer = await tauri.invoke('thread_settings_set', { threadId, reset: true })
        if (currentThreadId === threadId) threadModel = answer
        return
      }
      if (id === 'default') {
        if (!model) return
        await tauri.invoke('local_mode_set_default_model', { provider: model.provider, model: model.model })
        if (inventory) inventory = { ...inventory, default_provider: model.provider, default_model: model.model }
        return
      }
      const projectId = modelProject
      if (!projectId || !model) return
      const settings = await tauri.invoke('project_settings', { projectId })
      await tauri.invoke('project_settings_save', { projectId, settings: { ...settings, model } })
      // The thread now runs its project's model, so it reads as the project's.
      const answer = await tauri.invoke('thread_settings_set', { threadId, seed: { project_id: projectId, model, permission_mode: settings?.permissionMode ?? null, machine: settings?.machine ?? null } })
      if (currentThreadId === threadId) threadModel = answer
      await refreshProjects()
    } catch (_) {
      accountStatus = 'Muniment could not save the model choice. Try again.'
    } finally { modelChoicePending = false }
  }

  function prepareRetry(run, chooseModel = false) {
    if (active || (draft.trim() && draft !== run.prompt) || selectedFiles.length || !run.prompt?.trim()) return
    draft = run.prompt
    // The tools warning shows only when the failed turn ran a tool.
    const ranTools = !!run.toolActivity?.length || (run.receipt?.tools ?? []).some((tool) => Number(tool?.calls) > 0)
    retryReview = { threadId: currentThreadId, chooseModel, ranTools, previousProvider: chipModel?.provider, previousModel: chipModel?.model }
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
  // The names the user gave providers, such as a custom endpoint, for the provenance line.
  const providerNames = $derived(Object.fromEntries((inventory?.providers ?? []).filter((provider) => provider.id?.startsWith('custom-') && provider.name).map((provider) => [provider.id, provider.name])))
  let turnExtensions = $state({ enabled: [], blocked: [], automatic: null })
  function selectTask(next) {
    if (next === currentThreadId) return
    taskDrafts.set(currentThreadId, { text: draft, files: selectedFiles, extensions: turnExtensions, creation: pendingCreation })
    const saved = taskDrafts.get(next) || (currentThreadId === null && next ? taskDrafts.get(null) : null)
    draft = saved?.text || ''
    selectedFiles = saved?.files || []
    turnExtensions = saved?.extensions || { enabled: [], blocked: [], automatic: null }
    pendingCreation = saved?.creation || null
    currentThreadId = next
  }
  let draft = $state('')
  let selectedFiles = $state([])
  let submitError = $state('')
  // Every writer replaces the array, and a changed message or run is a new object.
  let messages = $state.raw([])
  const messageKeys = $derived(transcriptKeys(messages))
  let active = $state(null)
  // The run the window left on another thread, as { threadId, runId }. The runtime
  // keeps driving it, and the controller clears it when an event settles it.
  let backgroundRun = $state(null)
  // A submission waiting for its run id, a resume or a recovery keeps the window on its thread.
  const runLocked = $derived(!!active && (active.id === 'pending' || active.phase === 'resuming' || active.phase === 'recovering'))
  const switchLocked = $derived(runLocked || threadSwitching)
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
  const fileContext = $derived(agentsOpen ? {catalog:'agents'} : artifactsOpen ? {catalog:'artifacts'} : projectsOpen ? (settingsProject && sidebarProjectById[settingsProject]?.kind === 'project' ? {projectId:settingsProject} : catalogProject ? {projectId:catalogProject} : {catalog:'projects'}) : selectedAgent ? {agentId:selectedAgent} : currentCreation?.kind === 'artifact' && currentCreation.resultId ? {artifactId:currentCreation.resultId} : {threadId:currentThreadId, projectId:projectCatalog.threads[currentThreadId] || null, threadTitle:currentThreadTitle})
  let expandedProjects = $state(new Set())
  let projectForm = $state(null)
  let projectName = $state('')
  let projectBusy = $state(false)
  let projectError = $state('')
  const projectRows = $derived(Object.entries(projectCatalog.projects).sort((a, b) => a[1].localeCompare(b[1])))
  // Projects another fleet member owns, each with its owner. Their files
  // stay on that member.
  const remoteProjectRows = $derived(Object.entries(projectCatalog.remote ?? {}).map(([id, project]) => [id, project.name, projectOwner(project, swarm, runtimeList)]).sort((a, b) => a[1].localeCompare(b[1])))
  const regularThreads = $derived(listedThreads(threadSummaries, { agentThreads: agentThreadMap, groupThreads: swarm.groups.map(group => group.thread_id), creations }))
  let threadOrganization = $state(readThreadOrganization())
  let threadSearch = $state('')
  let threadSearchInput = $state()
  let threadIndexFailed = $state(false)
  let rowRename = $state(null)
  let rowRenameInput = $state()
  let rowRenamePending = $state(false)
  // The sidebar's Projects list from core's `sidebar::arrange`. Each answer
  // replaces the last whole. Reading a thread changes its status, not its
  // place, so rows move only on new activity.
  let sidebarArrangement = $state.raw(emptySidebar())
  let readState = $state.raw(readMarks())
  let archivedOpen = $state(false)
  let needsYouItems = $state.raw([])
  let sidebarRequest = 0
  // A new thread shows in Projects once its first message is sent: an empty
  // draft, such as a new agent's chat, is no thread yet. A new agent's chat
  // lists once it holds a draft, while its runtime thread is still fresh.
  const listedThreadId = $derived(freshThread && !agentChat?.record ? null : currentThreadId)
  const showFreshThread = $derived(freshThread && messages.length > 0 && !selectedAgent && !currentCreation && !threadSearch.trim())
  const sidebarRuns = $derived(runStates(activityRuns))
  const sidebarFeed = $derived(sidebarInput({
    summaries: threadSummaries,
    agentListing, agents: roster.filter(agent => !catalogArchived('agent', agent)), groups: swarm.groups, agentChats: agentChatRows, catalog: projectCatalog, remote: remoteProjectRows,
    organization: threadOrganization, marks: readState, needsYou: needsYouItems, current: listedThreadId,
    working: [...sidebarRuns.working, ...(runningThreadId ? [runningThreadId] : [])], failed: sidebarRuns.failed,
    query: threadSearch,
    machineOf: (runtimeId) => pairedRuntimes(runtimeList).length ? machineName(runtimeList.runtimes.find(runtime => runtime.id === (runtimeId ?? LOCAL_RUNTIME_ID)) ?? { id: runtimeId }) : null,
  }))
  // Core answers identical input with the same sections, so the same input is not asked twice in a row.
  let sidebarAsked = ''
  $effect(() => {
    const input = sidebarFeed
    if (!tauri) return
    const asked = JSON.stringify(input)
    if (asked === sidebarAsked) return
    sidebarAsked = asked
    const request = ++sidebarRequest
    Promise.resolve(arrangeSidebar(tauri, input)).then((next) => {
      if (request !== sidebarRequest || !next) return
      sidebarArrangement = next
    }).catch(() => { if (request === sidebarRequest) sidebarAsked = '' })
  })
  // The open thread is read while it shows, and again as its replies land.
  $effect(() => {
    const threadId = listedThreadId
    const updatedAt = Date.parse(threadSummaries.find(summary => summary.threadId === threadId)?.updatedAt ?? '')
    void agentListing
    // A mark already newer than the thread's last change stays, so the sidebar input stays the same.
    if (threadId) untrack(() => { if (!(Date.parse(readState.read[threadId] ?? '') >= updatedAt)) readState = markRead(readState, threadId) })
  })
  const sidebarProjectById = $derived(Object.fromEntries(sidebarArrangement.projects.map(project => [project.id, project])))
  // The machine that holds each project another member shares.
  const projectOwners = $derived(Object.fromEntries(remoteProjectRows.map(([id, , owner]) => [id, owner])))
  const shortcutThreads = $derived(shownRows(sidebarArrangement, { expanded: expandedProjects, fresh: showFreshThread }).map(row => ({ threadId: row.thread_id })))
  const freshThreadPosition = $derived(shortcutThreads.findIndex(thread => thread.threadId === null))
  let moreThreads = $state(false)
  let loadingOlderThreads = $state(false)
  let currentThreadId = $state(null)
  // The runtime reports how full the current thread's context is. The meter
  // reads it again when the thread changes, a reply ends or a compaction ends.
  let contextUsage = $state(null)
  let compacting = $state(false)
  let compactError = $state('')
  let contextRequest = 0
  const replyRunning = $derived(!!active)
  $effect(() => { void remoteRuntimeId; void auth.name; if (!replyRunning) untrack(() => { void refreshComposerLimits() }) })
  async function refreshContextUsage() {
    const request = ++contextRequest
    const threadId = currentThreadId
    if (!threadId) { contextUsage = null; return }
    try {
      const next = await tauri.invoke('thread_context_usage', { threadId })
      if (request === contextRequest) contextUsage = next
    } catch (_) { if (request === contextRequest) contextUsage = null }
  }
  $effect(() => { void currentThreadId; if (!replyRunning) void refreshContextUsage() })
  // The composer's permission mode. Core lists the modes of the thread's
  // machine, or of the picked machine for a new thread, and answers the
  // open thread's mode. A new thread shows the default thread mode checked.
  // A mode picked on it waits for its first message, which carries it, and
  // core sets it before the run starts.
  let modeChip = $state()
  let modeListing = $state(null)
  let modesRevision = $state(0)
  let threadMode = $state(null)
  let pendingMode = $state(null)
  // Where the new thread's mode comes from: 'project' for its project's
  // default, 'thread' for a pick on it.
  let pendingModeLayer = $state(null)
  let modeRequest = 0
  const composerThreadId = $derived(freshThread ? null : currentThreadId)
  const composerRuntime = $derived(threadMachine ?? runtimeList?.selected ?? LOCAL_RUNTIME_ID)
  // The modes load again once Home exists and the runtime answers, and a
  // runtime that lists none yet is asked again.
  $effect(() => {
    const runtime = composerRuntime
    void modesRevision
    void onboarding.name
    void desktopClientStatus?.connected
    untrack(() => { if (modeListing?.runtime !== runtime) modeListing = null })
    return watchModes(tauri, runtime, (listing) => { modeListing = { runtime, listing } })
  })
  async function refreshThreadMode() {
    const request = ++modeRequest
    const threadId = composerThreadId
    const runtime = composerRuntime
    if (!threadId || !modeListing) { threadMode = null; return }
    try {
      const answer = await agentRequest(tauri, runtime, 'thread.permission_mode', { thread_id: threadId })
      if (request === modeRequest) threadMode = answer
    } catch (_) { if (request === modeRequest) threadMode = null }
  }
  // A plan approval switches the thread's mode during a reply, so the mode is read again when a reply starts or ends.
  $effect(() => { void composerThreadId; void modeListing; void replyRunning; void refreshThreadMode() })
  $effect(() => { if (composerThreadId) untrack(() => { pendingMode = null; pendingModeLayer = null }) })
  const composerMode = $derived(!modeListing ? null
    : composerThreadId ? (threadMode?.thread_id === composerThreadId ? findMode(modeListing.listing, threadMode.mode) : null)
      : findMode(modeListing.listing, pendingMode ?? modeListing.listing.default_thread_mode))
  // Group threads and threads another machine runs take no mode from here.
  const composerModeShown = $derived(!!composerMode && !threadGroup && !threadPostTarget)
  // Where a new thread's machine comes from: its project's default, a pick
  // in the machine menu, or the default.
  let machineLayer = $state(null)
  const machineSource = $derived(layerLabel(machineLayer ?? 'default', { projectName: projectCatalog.projects[selectedProject] ?? '' }))
  // The mode's layer, as core names it, beside the mode in its menu.
  const modeSource = $derived.by(() => {
    if (composerThreadId) {
      const projectId = threadMode?.project_id
      return layerLabel(threadMode?.layer ?? (threadMode?.source === 'agent' ? 'agent' : threadMode?.source === 'thread' ? 'thread' : 'default'), { projectName: projectCatalog.projects[projectId] ?? '', agentName: threadAgent?.name ?? '' })
    }
    if (!pendingMode) return 'default'
    return layerLabel(pendingModeLayer === 'project' ? 'project' : 'thread', { projectName: projectCatalog.projects[selectedProject] ?? '' })
  })
  async function setThreadMode(threadId, mode) {
    ++modeRequest
    try {
      const answer = await agentRequest(tauri, composerRuntime, 'thread.permission_mode_set', { thread_id: threadId, mode })
      if (threadId === composerThreadId) threadMode = answer
    } catch (error) {
      submitError = String(error?.message ?? error)
    }
  }
  function closeModes(focus = true) {
    modeOpen = false
    if (focus) void tick().then(() => modeChip?.focus())
  }
  function chooseMode(id, focus = true) {
    closeModes(focus)
    const mode = findMode(modeListing?.listing, id)
    if (!mode || mode.available === false || id === composerMode?.id) return
    if (!composerThreadId) { pendingMode = id === modeListing.listing.default_thread_mode ? null : id; pendingModeLayer = 'thread'; return }
    void setThreadMode(composerThreadId, id)
  }
  // The plan a composer thread's plan gate waits on, from core's `thread.plan`.
  let composerPlan = $state(null)
  const planGateId = $derived(messages.findLast((message) => message.run?.phase === 'pending-permission' && isPlanGate(message.run.pendingPermission))?.run.pendingPermission.gateId ?? null)
  $effect(() => {
    const gateId = planGateId
    const threadId = composerThreadId
    const runtime = composerRuntime
    if (!gateId || !threadId) { composerPlan = null; return }
    let alive = true
    agentRequest(tauri, runtime, 'thread.plan', { thread_id: threadId })
      .then((answer) => { if (alive) composerPlan = answer?.plan ? { gateId, text: answer.plan.plan ?? '' } : null })
      .catch(() => { if (alive) composerPlan = null })
    return () => { alive = false }
  })
  // Shift+Tab in the composer moves to the next mode that can be chosen.
  function cycleMode() {
    const next = nextMode(modeListing?.listing, composerMode?.id)
    if (next) chooseMode(next.id, false)
  }
  async function compactThread(instructions, fromDraft = false) {
    if (compacting || active) return
    if (!currentThreadId) { submitError = 'Start a conversation before you compact it.'; return }
    compacting = true
    compactError = ''
    submitError = ''
    try {
      await tauri.invoke('thread_compact', { threadId: currentThreadId, instructions })
      if (fromDraft) draft = ''
      openComposerPanel(null)
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      if (contextOpen) compactError = message
      else submitError = message
    } finally {
      compacting = false
      void refreshContextUsage()
    }
  }
  // `/compact` runs here and sends no message to the model.
  // A creation chat shows its template as a chip and sends the full creation
  // prompt with whatever the user typed beside it.
  function outgoingDraft() {
    return pendingCreation ? creationPrompt(pendingCreation.kind, pendingCreation, draft) : draft
  }

  function sendOrRun() {
    if (agentChat) { sendComposerToAgentChat(); return }
    const command = compactCommand(draft)
    if (command) void compactThread(command.instructions, true)
    else if (threadPostTarget) void postToThread(threadPostTarget)
    else if (freshThread && offloadTarget) void offloadThread()
    else chatController.send()
  }
  // A group thread, or a replica of another machine's agent thread, takes a
  // message through `thread.post`. Core hands it to the group or to the
  // agent's primary machine.
  const threadPostTarget = $derived(postTarget({ threadId: freshThread ? null : currentThreadId, threadRuntime: threadMachine, groups: swarm.groups, agent: threadAgent, swarm, owner: currentSummary?.owner_runtime, replica: !!currentSummary?.replica }))
  // The open thread's summary, and the fleet member that owns it when that
  // is another machine.
  const currentSummary = $derived(freshThread || !currentThreadId ? null : threadSummaries.find((summary) => summary.threadId === currentThreadId) ?? null)
  const currentOwner = $derived(currentSummary ? threadOwner(currentSummary, swarm, runtimeList) : null)
  // A message queued for the open thread's offline owner, or for the
  // machine a new thread started on.
  let pendingOffload = $state(null)
  const queuedOp = $derived(pendingOffload
    ? fleet.state.ops.find((op) => op.op_id === pendingOffload.opId && op.state === 'queued') ?? null
    : queuedFor(fleet.state.ops, freshThread ? null : currentThreadId, threadAgent?.replica ? threadAgent.id : null))
  // A replica's history travels from its owner on open: core pins the
  // thread and answers `fetching` until it holds every event.
  let historyFetch = $state(null)
  const replicaKey = $derived(currentSummary?.replica ? `${currentSummary.runtimeId ?? LOCAL_RUNTIME_ID}\u001f${currentSummary.threadId}` : null)
  $effect(() => {
    const key = replicaKey
    historyFetch = null
    if (!key || !tauri) return
    const [runtime, threadId] = key.split('\u001f')
    let alive = true
    let timer = null
    let waited = false
    const fetchHistory = async () => {
      try {
        const answer = await agentRequest(tauri, runtime, 'thread.history_fetch', { thread_id: threadId })
        if (!alive) return
        historyFetch = answer
        if (answer?.status === 'held') { if (waited) void chatController.refreshOpenThread(); return }
        waited = true
        timer = setTimeout(fetchHistory, 2000)
      } catch (_) { if (alive) historyFetch = null }
    }
    untrack(() => { void fetchHistory() })
    return () => { alive = false; clearTimeout(timer) }
  })
  const composerFleetLine = $derived(pendingOffload && !queuedOp
    ? `Starting on ${pendingOffload.name}. It opens here once it syncs.`
    : historyLine(historyFetch, currentOwner?.name ?? historyFetch?.owner_name ?? ''))
  // The machine a new thread starts on when it is a fleet member this app
  // does not reach itself, such as a container.
  let offloadTarget = $state(null)
  const fleetTargets = $derived(offloadTargets(swarm, runtimeList))
  async function offloadThread() {
    const text = draft.trim()
    const target = offloadTarget
    if (!text || posting || !target) return
    posting = true
    submitError = ''
    try {
      const answer = await tauri.invoke('fleet_thread_create', { runtimeId: target.coreId, text, permissionMode: pendingMode ?? null })
      fleet.forwarded(answer)
      pendingOffload = { threadId: answer?.thread_id, opId: answer?.op_id ?? null, name: target.name }
      draft = ''
      await chatController.refreshThreads()
    } catch (error) {
      submitError = String(error?.message ?? error)
    } finally {
      posting = false
    }
  }
  // An offloaded thread opens once its owner's sync brings it here.
  $effect(() => {
    const pending = pendingOffload
    if (!pending?.threadId || !threadSummaries.some((summary) => summary.threadId === pending.threadId)) return
    untrack(() => {
      pendingOffload = null
      offloadTarget = null
      void chatController.openThread(pending.threadId)
    })
  })
  // An agent whose machine has been away a day moves here by force.
  async function moveAgentHere() {
    const request = moveRequest({ agent: threadAgent, to: swarm.self[LOCAL_RUNTIME_ID], swarm, force: true })
    if (request.error) { submitError = request.error; return }
    try {
      await agentRequest(tauri, request.runtime, 'agent.move', request.body)
      await refreshAgents()
    } catch (error) {
      submitError = String(error?.message ?? error)
    }
  }
  let posting = $state(false)
  async function postToThread(target, text = draft.trim()) {
    if (!text || posting) return false
    posting = true
    submitError = ''
    try {
      const answer = await agentRequest(tauri, target.runtime, 'thread.post', { ...target.body, text })
      fleet.forwarded(answer)
      draft = ''
      await chatController.refreshOpenThread()
      return true
    } catch (error) {
      submitError = String(error?.message ?? error)
      return false
    } finally {
      posting = false
    }
  }
  // Messages posted on swarm members, for the open thread.
  let threadPosts = $state({ threadId: null, posts: [] })
  const currentPosts = $derived(threadPosts.threadId === currentThreadId ? visiblePosts(threadPosts.posts) : [])
  const postSections = $derived(new Map(meetingSections(currentPosts).map(({ post, section }) => [post.message_id, section])))
  const transcriptItems = $derived(timeline(messages, currentPosts))
  // The rendered part of the transcript. src/lib/transcript-window.js bounds it.
  let transcriptWindow = $state.raw(tailWindow())
  const shownItems = $derived(windowItems(transcriptItems, transcriptWindow))
  // Approvals waiting in the open thread. Each shows as an approval card at
  // the end of the transcript in place of the run's own gate.
  const openApprovals = $derived(threadApprovals(swarm.approvals, freshThread ? null : currentThreadId))
  // A catalog or a page covers the thread, so the title bar names it.
  const catalogTitle = $derived(agentsOpen ? 'Agents' : artifactsOpen ? 'Artifacts' : projectsOpen ? (settingsProject ? sidebarProjectById[settingsProject]?.name ?? 'Projects' : 'Projects') : fullPage === 'usage' ? 'Usage' : fullPage === 'notifications' ? 'Notifications' : '')
  let currentThreadTitle = $derived(agentChat ? agentChatTitle(agentChat.record) : threadAgent?.name || threadGroup?.name || (currentCreation ? creationTitle(currentCreation, threadSummaries, artifactItems.find(item=>item.id===currentCreation.resultId)?.name) : threadSummaries.find(({ threadId }) => threadId === currentThreadId)?.title || threadTitle(messages)))
  let freshThread = $state(false)
  // The machine the open thread lives on. A new thread has none until its
  // first message, and its machine comes from the composer's picker.
  const threadMachine = $derived(!freshThread && currentThreadId
    ? threadSummaries.find((summary) => summary.threadId === currentThreadId)?.runtimeId ?? runtimeList?.selected ?? null
    : null)
  // The open thread's machine while it cannot take a message, such as one
  // that is offline or revoked this device. Each such state names a next step.
  const unreachableMachine = $derived.by(() => {
    if (!threadMachine || threadMachine === LOCAL_RUNTIME_ID) return null
    const machine = runtimeList?.runtimes?.find((runtime) => runtime.id === threadMachine)
    return machine && statusHint(machine) ? machine : null
  })
  let threadSwitching = $state(false)
  const runningThreadId = $derived(backgroundRun?.threadId ?? (active ? currentThreadId : null))
  // A running agent or artifact chat marks its own sidebar row, or its collapsed group.
  const backgroundRunAgent = $derived(agentThreadMap[backgroundRun?.threadId] ?? null)
  const backgroundRunArtifact = $derived(artifactChats.find(item => item.threadId && item.threadId === backgroundRun?.threadId)?.threadId ?? null)
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
  // Derived flags, so a streamed chunk that changes the run leaves the composer's controls untouched.
  const voiceDisabled = $derived(!!active || dictationFinishing)
  const resumingReply = $derived(active?.phase === 'resuming')
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
  const readChangedFiles = createChangedFiles()
  const changed = $derived(readChangedFiles(messages))
  // Review changes and a rewind open over the thread. A rewind's report stays
  // above the composer until it is dismissed or the thread changes.
  let reviewing = $state(null)
  let rewinding = $state(null)
  let rewindNotice = $state(null)
  const loadReviewChanges = () => import('./files/ReviewChanges.svelte')
  const loadRewindDialog = () => import('./activity/RewindDialog.svelte')
  // Each user message's turn: its run, or the run that answers it.
  const userTurnRuns = $derived(new Map(messages.flatMap((message, index) => message.role === 'user' ? [[message, message.id ?? messages[index + 1]?.run?.id ?? null]] : [])))
  const rewindBlocked = $derived(rewindUnavailable({ owner: currentOwner, replica: !!currentSummary?.replica, group: !!threadGroup, running: !!active }))
  const runMachine = $derived(runMachineName(currentOwner, runtimeList, threadMachine))
  // The settled turns Review changes reads, newest first.
  const reviewTurns = $derived(messages.flatMap((message, index) => message.role === 'assistant' && message.run?.id && settledPhases.has(message.run.phase)
    ? [{ runId: message.run.id, prompt: promptFolders(message.run.prompt || (messages[index - 1]?.role === 'user' ? messages[index - 1].text : '')).text }] : []).reverse().slice(0, 20))
  $effect(() => { void currentThreadId; untrack(() => { rewindNotice = null; reviewing = null; rewinding = null }) })
  async function finishRewind(result) {
    const turn = rewinding
    rewinding = null
    rewindNotice = rewindOutcome(result)
    if (turn?.prompt && !draft.trim()) draft = turn.prompt
    await chatController.refreshOpenThread(true)
    composer?.focus()
  }
  $effect(() => {
    const files = changed.filter(file => !file.deleted && /\.html?$/i.test(file.path))
    const threadId = currentThreadId
    if (active || !threadId || !files.length) return
    void Promise.allSettled(files.map(file => tauri.invoke('artifact_from_file', {threadId, path:file.path})))
  })
  function openFile(file) { requestedWorkspaceFile = { ...file, name: file.name || fileName(file.path) }; showBrowser(`file:${file.path}`) }
  // Chat events leave out tool output bodies and applied diffs. The runtime
  // reads one from the run's record when its detail opens.
  const runDetail = (runId, effectId, kind) => tauri.invoke('chat_run_detail', { runId, effectId, kind })
  let appliedDiffReads = $state({})
  function readAppliedDiff(runId, effectId, retry = false) {
    const key = `${runId}\n${effectId}`
    if (appliedDiffReads[key] && !retry) return
    appliedDiffReads[key] = runDetail(runId, effectId, 'code_diff').then((detail) => detail.diff)
  }
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
  let pairingRequests = $state([])
  const pairingTimers = new Map()
  let desktopClientStatus = $state(null)
  let desktopClientStatusVersion = 0
  let runtimeNotice = $state(null)
  let backgroundServiceNoticeVisible = $derived(runtimeNotice?.visible === true)
  let foregroundOnlyDismissed = $state(false)
  let foregroundOnlyNotice = $derived(foregroundOnlyDismissed ? null : runtimeNotice?.foregroundOnly ?? null)
  // Each time the runtime falls back to running as the app's child is a new
  // cause. The close control keeps it away for the rest of the launch.
  let foregroundOnlyCause = 0
  let foregroundOnlyWas = false
  $effect(() => {
    const on = !!runtimeNotice?.foregroundOnly
    const notice = foregroundOnlyNotice
    untrack(() => {
      if (on && !foregroundOnlyWas) foregroundOnlyCause += 1
      foregroundOnlyWas = on
      if (!notice) { composerNotices.withdraw('background-service'); return }
      composerNotices.show({
        id: 'background-service', cause: foregroundOnlyCause, text: notice.text, label: 'Background service', testid: 'foreground-only-notice',
        action: { label: notice.control, run: () => backgroundServiceNotice.openLoginItems() },
        dismissLabel: 'Dismiss background service notice', ondismiss: () => { foregroundOnlyDismissed = true },
      })
    })
  })
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
  // ⌘K opens the command palette everywhere but a focused terminal.
  const searchKeyShortcut = paletteShortcut()
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
  const transcriptView = createTranscriptWindow({
    tick,
    readThread: () => thread,
    readLength: () => transcriptItems.length,
    readPinned: () => pinned,
    readWindow: () => transcriptWindow,
    onWindow: (next) => { transcriptWindow = next },
    onScrollTop: (top) => transcriptController.syncScrollTop(top),
  })
  function transcriptScrolled() {
    handleThreadScroll()
    void transcriptView.scrolled()
  }
  async function jumpToLatest() {
    transcriptWindow = tailWindow()
    await tick()
    scrollToLatest()
  }
  $effect(() => { void currentThreadId; untrack(() => { transcriptWindow = tailWindow() }) })
  $effect(() => { void transcriptItems.length; untrack(() => { void transcriptView.grew() }) })

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
  const toggleRecordPanel = () => { if (!featureFlags.companyRecord) return; artifactsOpen = false; browserPanel = null; projectsOpen = false; agentsOpen = false; fullPage = null; agentProfileOpen = false; railController.toggle('record') }
  const closeRail = () => railController.close()
  // Ask puts the open view's SQL into the composer as a fenced block, so the reply starts from what the person sees.
  function askAboutView(sql) {
    if (!sql) return
    const fence = '```sql\n' + sql + '\n```'
    draft = draft.trim() ? `${draft.trimEnd()}\n\n${fence}\n` : `${fence}\n`
    void tick().then(() => composer?.focus())
  }
  const toggleRecordMaximized = () => railController.toggleMaximized()
  // Settings → Companies opens the record panel on the sample or on one source's connect step.
  let recordRequest = $state(null)
  function openRecordFromSettings(request) {
    closeSettings()
    recordRequest = { ...request }
    if (!recordPanelOpen) toggleRecordPanel()
  }

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
    // A thread another machine runs that the list did not mark yet takes
    // the message as a post, which core sends on to that machine.
    postElsewhere: (text) => !freshThread && currentThreadId ? postToThread({ runtime: threadMachine || LOCAL_RUNTIME_ID, body: { thread_id: currentThreadId } }, text) : false,
    invoke: (command, args) => tauri.invoke(command, ...(args === undefined ? [] : [args])),
    onPosted: (threadId, posts) => { threadPosts = { threadId, posts } },
    prepareSubmit: async ({ prompt }) => {
      if (pendingCreation) {
        const plan = await tauri.invoke('creation_save', { creation: pendingCreation })
        creationsRevision += 1
        creations = [...creations.filter(item => item.threadId !== plan.threadId), plan]
        pendingCreation = null
      }
      await composerExtensions?.prepare(prompt)
    },
    onSubmitted: () => composerExtensions?.submitted(),
    listen: (...args) => window.__TAURI__?.event?.listen(...args),
    readMessages: () => messages,
    readActive: () => active,
    readAnnounced: () => announcedRun,
    readDraft: outgoingDraft,
    readFiles: () => selectedFiles,
    readThinkingLevel: () => chipThinking?.level ?? null,
    readPermissionMode: () => composerThreadId ? null : pendingMode,
    readPendingModel: () => currentThreadId ? null : pendingModel,
    readThreadId: () => currentThreadId,
    readThreadSummaries: () => threadSummaries,
    readRuntime: () => composerRuntime,
    blocked: () => dictationBusy() || runtimeUpgradePending() || !!unreachableMachine || modelChoicePending || compacting || !!retryReview?.chooseModel,
    onMessages: (next) => { messages = next },
    onActive: (next) => { const finished = !!active && !next; active = next; if (finished) void revealSavedArtifact() },
    onAnnounce: (next) => { announcedRun = next },
    onDraft: (next) => { draft = next },
    onFiles: (next) => { selectedFiles = next },
    onSubmitError: (next) => { submitError = next },
    onCancelError: (next) => { cancelError = next },
    onQueueError: (next) => { queueError = next },
    onThreadListError: (next, action) => { threadListError = next; threadListErrorAction = action },
    onHistoryError: (next, action) => { historyError = next; historyErrorAction = action },
    onThreadSummaries: (next) => { threadSummaries = dedupeThreads(next, swarm) },
    onMoreThreads: (next) => { moreThreads = next },
    readProject: () => selectedProject,
    readAgent: () => selectedAgent,
    onThreadSelected: (next) => { selectTask(next); profileOnlyAgent = null; selectedAgent = agentThreadMap[next] ?? startingAgent ?? null; if (!selectedAgent) agentProfileOpen = false; void refreshProjects(next); void refreshAgents(); void refreshCreations() },
    onThreadSwitch: (next) => { threadSwitching = next },
    onFreshThread: (next) => { freshThread = next },
    onHistoryStart: () => { expandedReceipts = new Set(); parallelTools = new Map() },
    onHistoryLoaded: () => { pinned = true; transcriptWindow = tailWindow() },
    onFollow: followNewContent,
    onFocus: () => tick().then(() => composer?.focus()),
    onSend: () => { retryReview = null },
    onSubmission: (detail) => window.dispatchEvent(new CustomEvent('muniment-chat-submit', { detail })),
    onSignInLink: (link) => { if (auth.name === 'signing-in') auth = { ...auth, link } },
    onBackgroundRun: (next) => { backgroundRun = next },
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
    const needsIndex = projectsOpen || [...expandedProjects].some((id) => !openedForThread.has(id)) || threadSearch.trim() || archivedOpen || Object.entries(threadOrganization)
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
    } catch (error) { projectError = `Projects could not be loaded. ${String(error?.message ?? error)}` }
  }

  async function showProjects() {
    settingsProject = null
    projectsOpen = true; agentsOpen = false; fullPage = null; artifactsOpen = false; agentProfileOpen = false; closeRail()
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
    openedForThread = new Set([...openedForThread].filter((id) => id !== projectId))
    expandedProjects = next
  }
  // The open thread's project opens once, so the open thread's row shows.
  // A project the user closes after that stays closed. A project the window
  // opened this way loads no older page of threads; one the user opens does.
  let expandedFor = null
  let openedForThread = $state(new Set())
  $effect(() => {
    const threadId = listedThreadId
    const project = threadId && sidebarArrangement.projects.find((entry) => entry.kind !== 'thread' && entry.threads.some((row) => row.thread_id === threadId))
    if (!project || expandedFor === threadId) return
    expandedFor = threadId
    untrack(() => {
      if (expandedProjects.has(project.id)) return
      openedForThread = new Set([...openedForThread, project.id])
      expandedProjects = new Set([...expandedProjects, project.id])
    })
  })

  // A new thread in a project.
  async function newProjectThread(projectId) {
    if (switchLocked || projectBusy) return
    await newSidebarThread(projectId)
  }

  // A project's settings take the thread's place.
  async function showProjectSettings(projectId) {
    settingsProject = projectId
    projectsOpen = true; agentsOpen = false; fullPage = null; artifactsOpen = false; agentProfileOpen = false; closeRail()
    await refreshProjects()
  }

  // A trigger makes the project an agent: a new agent chat in the project
  // drafts one that works in the project's folder, and its home thread is
  // where runs post.
  async function addProjectTrigger(project) {
    await openAgentChat({ projectId: project.id })
  }

  async function saveProject() {
    if (remoteSelected(runtimeList)) await onThisComputer()
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
      if (creating) await selectProject(id)
    } catch (error) { projectError = String(error); projectBusy = false }
  }

  async function openProjectFolder(projectId) {
    try { await tauri.invoke('project_open', { projectId }); projectError = '' }
    catch (_) { projectError = 'The project folder could not be opened.' }
  }

  async function newSidebarThread(projectId = null) {
    artifactsOpen = false
    if (switchLocked || projectBusy) return
    selectedProject = projectId
    projectsOpen = false
    agentsOpen = false; fullPage = null
    agentProfileOpen = false
    selectedAgent = null
    // A new thread starts with its project's defaults, on this computer
    // unless the project names a machine. The composer's pickers change them.
    const defaults = (projectId && projectCatalog.settings?.[projectId]) || {}
    const machine = defaults.machine ? runtimeList?.runtimes?.find((runtime) => runtime.id === defaults.machine && !runtime.local && runtime.status === 'online') : null
    if (machine) { if (runtimeList?.selected !== machine.id) await selectRuntime(machine) }
    else if (remoteSelected(runtimeList)) await selectRuntime(runtimeList.runtimes.find((runtime) => runtime.local))
    machineLayer = machine ? 'project' : null
    if (!await chatController.newThread()) return
    // Core started the thread with its project's model; the mode waits for
    // the first message.
    if (defaults.permissionMode) { pendingMode = defaults.permissionMode; pendingModeLayer = 'project' }
    void refreshThreadModel()
    threadSearch = ''
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

  // Short notices over the composer, one at a time. Each shows once per
  // launch for each cause, so a state read again does not bring it back.
  const composerNotices = createComposerNotices()
  $effect(() => {
    const text = speechInstallNotice
    untrack(() => {
      if (text) composerNotices.show({ id: 'speech-install', cause: text, text, dismissLabel: 'Dismiss voice notice' })
      else composerNotices.withdraw('speech-install')
    })
  })
  // A shortcut that fails again after it worked is a new cause.
  let voiceShortcutFailures = 0
  $effect(() => {
    const failed = globalVoiceError
    untrack(() => {
      if (!failed) { composerNotices.withdraw('voice-shortcut'); return }
      voiceShortcutFailures += 1
      composerNotices.show({ id: 'voice-shortcut', cause: voiceShortcutFailures, text: 'The system-wide voice shortcut is unavailable. Voice remains available from the button.', dismissLabel: 'Dismiss voice shortcut notice' })
    })
  })

  const entitlementToast = createEntitlementToast({
    listen: (...args) => window.__TAURI__?.event?.listen(...args),
    notices: composerNotices,
  })

  const backgroundServiceNotice = createBackgroundServiceNotice({
    invoke: (...args) => tauri.invoke(...args),
    listen: (...args) => window.__TAURI__?.event?.listen(...args),
    onChange: (notice) => { runtimeNotice = notice },
  })

  // Once Home is set, the window reads it beside the runtime's start, so a
  // pending or refused macOS folder prompt names itself in the sidebar and
  // on Usage instead of leaving this computer silent.
  let folderAccess = $state(null)
  let folderAccessWatch = null
  const folderAccessHome = $derived(tauri && onboarding.name === 'complete' ? onboarding.homePath ?? '' : '')
  $effect(() => {
    const homePath = folderAccessHome
    if (!homePath) return
    const watch = createFolderAccessWatch({ invoke: (...args) => tauri.invoke(...args), homePath, onChange: (next) => { folderAccess = next } })
    folderAccessWatch = watch
    void watch.start()
    return () => {
      watch.stop()
      if (folderAccessWatch === watch) folderAccessWatch = null
      folderAccess = null
    }
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
    // A runtime that comes back may serve other models than the last read named.
    if (requestsRecovered && workspaceMode() && auth.name === 'local') void refreshInventory({ keep: true })
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
  // While rows are selected, a plain click adds or drops its row instead.
  function threadRowClick(event, threadId) {
    if (event.shiftKey || event.metaKey || event.ctrlKey || selectedThreadIds.size) {
      event.preventDefault()
      selectThread(threadId, event.shiftKey)
      return
    }
    projectsOpen = false
    agentsOpen = false; fullPage = null
    artifactsOpen = false
    clearThreadSelection()
    // The open thread's row is a button only while a catalog covers it, and
    // a click then uncovers the thread without opening it again.
    if (threadId === currentThreadId) return
    void chatController.openThread(threadId)
  }

  // The open thread's row is not a button, so a modifier click, or any click
  // while rows are selected, selects it.
  function currentRowClick(event, threadId) {
    if (!(event.shiftKey || event.metaKey || event.ctrlKey || selectedThreadIds.size)) return
    event.preventDefault()
    event.currentTarget.focus()
    selectThread(threadId, event.shiftKey)
  }

  // A row core arranged opens its thread. A group's thread opens the group,
  // and an agent's thread this window does not hold opens on the agent's machine.
  function sidebarRowClick(event, summary) {
    const row = summary.row
    const plain = event.shiftKey || event.metaKey || event.ctrlKey || selectedThreadIds.size
    if (!plain && row) {
      const group = swarm.groups.find((entry) => entry.thread_id === row.thread_id)
      if (group) { clearThreadSelection(); void openGroup(group).catch(error => { submitError = String(error) }); return }
      // A row that needs the user opens the thread that waits on them.
      if (row.status === 'needs_you' && row.thread_id) { clearThreadSelection(); void showThread(row.thread_id).catch(error => { submitError = String(error) }); return }
      const agent = row.agent && !threadSummaries.some((entry) => entry.threadId === row.thread_id) && !agentChatRows.some((chat) => chat.threadId === row.thread_id) ? roster.find((entry) => entry.id === row.agent.id) : null
      if (agent) { clearThreadSelection(); void openAgent(agent).catch(error => { submitError = String(error) }); return }
    }
    threadRowClick(event, summary.threadId)
  }

  // Each sidebar row names itself by its title and describes itself by its status, project, machine and time.
  let rowIds = 0
  const nextRowId = () => `sidebar-row-${++rowIds}`

  // The pages Search finds beside threads.
  const sidebarPages = $derived([
    { id: 'artifacts', label: 'Artifacts', icon: 'file-code', shortcut: artifactShortcut },
    { id: 'agents', label: 'Agents', icon: 'bot' },
    { id: 'new-agent', label: 'New agent', icon: 'plus' },
    { id: 'projects', label: 'Projects', icon: 'layout-dashboard' },
    ...(roster.length > 1 ? [{ id: 'group', label: 'New group chat', icon: 'users' }] : []),
    { id: 'notifications', label: 'Notifications', icon: 'bell' },
    { id: 'usage', label: 'Usage', icon: 'chart-column' },
    { id: 'settings', label: 'Settings', icon: 'settings', shortcut: settingsKeyShortcut },
  ])
  function openSidebarPage(id) {
    threadSearch = ''
    if (id === 'artifacts') void showArtifacts()
    else if (id === 'agents') void showAgents()
    else if (id === 'new-agent') void openAgentChat()
    else if (id === 'projects') void showProjects()
    else if (id === 'group') groupCreateOpen = true
    else if (id === 'settings') toggleSettings()
    else toggleFullPage(id)
  }

  // The command palette, ⌘K. Core ranks what it finds from the sidebar's
  // arrangement, the app's pages, the Settings pages and the actions that
  // apply to the open thread.
  let paletteOpen = $state(false)
  const threadMovable = (threadId) => !!threadId && !agentThreadMap[threadId] && !swarm.groups.some((group) => group.thread_id === threadId) && threadId !== runningThreadId
  function paletteFor(query) {
    const positions = new Map(shortcutThreads.slice(0, 9).map((row, index) => [row.threadId, index + 1]))
    const open = !freshThread && currentThreadId ? { thread_id: currentThreadId, pinned: !!threadOrganization[currentThreadId]?.pinned, movable: threadMovable(currentThreadId) } : null
    const machines = (runtimeList?.runtimes ?? []).filter((runtime) => runtime.local || runtime.status === 'online').length || 1
    return paletteInput({ query, sidebar: sidebarArrangement, positions, pages: settingsPages({ remote: !!remoteRuntime }), thread: open, machines, modes: composerModeShown })
  }
  function runPaletteItem(item) {
    if (item.kind === 'thread') {
      if (switchLocked) return
      projectsOpen = false; agentsOpen = false; fullPage = null; artifactsOpen = false
      if (settingsOpen) closeSettings()
      if (item.id !== currentThreadId || freshThread) void chatController.openThread(item.id)
      return
    }
    if (item.kind === 'page') {
      if (item.id.startsWith(SETTINGS_PAGE)) return openSettings(item.id.slice(SETTINGS_PAGE.length))
      if (item.id === 'settings') { if (!settingsOpen) toggleSettings(); return }
      return openSidebarPage(item.id)
    }
    if (item.id === 'new_thread') void newSidebarThread()
    else if (item.id === 'new_project') { if (sidebarCollapsed) toggleSidebar(); if (settingsOpen) closeSettings(); projectForm = 'new'; projectName = ''; projectsOpen = false }
    else if (item.id === 'new_agent') void openAgentChat()
    else if (item.id === 'move_to_project') openMoveDialog(currentThreadId)
    else if (item.id === 'pin' && currentThreadId) setThreadOrganization(currentThreadId, 'pinned')
    else if (item.id === 'switch_model') { if (settingsOpen) closeSettings(); void tick().then(() => togglePicker()) }
    else if (item.id === 'switch_machine') { if (settingsOpen) closeSettings(); void tick().then(() => openComposerPanel('machine')) }
    else if (item.id === 'toggle_permission_mode') cycleMode()
  }

  // Move to project: the thread menu, the palette and a row dropped on a
  // project move a thread into a project on this computer, or out to no
  // project, where it is a project of its own. The thread keeps its
  // conversation and runs next in the new project's folder with its
  // instructions and tools.
  let moveTarget = $state(null)
  let moveBusy = $state(false)
  let moveError = $state('')
  let moveNote = $state(null)
  const moveProjects = $derived([
    ...projectRows.map(([id, name]) => ({ id, name })),
    { id: null, name: 'No project' },
  ])
  function openMoveDialog(threadId) {
    closeThreadMenu()
    if (!threadMovable(threadId)) return
    const summary = threadSummaries.find((item) => item.threadId === threadId)
    moveError = ''
    moveTarget = { threadId, title: summary?.title || (threadId === currentThreadId ? currentThreadTitle : 'New thread') }
  }
  async function moveThread(threadId, projectId) {
    if (!threadMovable(threadId) || moveBusy) return false
    const from = projectCatalog.threads[threadId] ?? null
    if (from === projectId) return false
    moveBusy = true; moveError = ''
    try {
      await tauri.invoke('project_move_thread', { threadId, projectId })
      // A model the thread took from its old project follows the new one.
      if (projectId && threadId === currentThreadId && threadModel?.layer === 'project') {
        const defaults = projectCatalog.settings?.[projectId] ?? {}
        threadModel = await tauri.invoke('thread_settings_set', { threadId, seed: { project_id: projectId, model: defaults.model ?? null, permission_mode: defaults.permissionMode ?? null, machine: defaults.machine ?? null } })
      }
      await refreshProjects(threadId)
      moveTarget = null
      const name = projectCatalog.projects[projectId] ?? 'the project'
      moveCount += 1
      moveNote = { threadId, move: moveCount, seen: threadId === currentThreadId, text: projectId ? `Now uses ${name} instructions and tools.` : 'Moved out of its project.' }
      return true
    } catch (error) {
      moveError = String(error?.message ?? error)
      if (!moveTarget) submitError = moveError
      return false
    } finally { moveBusy = false }
  }
  // A thread row dragged onto a project row moves it there.
  const THREAD_DRAG = 'application/x-muniment-thread'
  function threadDragStart(event, threadId) {
    if (!threadMovable(threadId) || !event.dataTransfer) { event.preventDefault(); return }
    event.dataTransfer.setData(THREAD_DRAG, threadId)
    event.dataTransfer.effectAllowed = 'move'
  }
  // The note shows on the moved thread until the user leaves it.
  $effect(() => {
    const threadId = currentThreadId
    untrack(() => {
      if (!moveNote) return
      if (moveNote.threadId === threadId) moveNote.seen = true
      else if (moveNote.seen) moveNote = null
    })
  })
  let moveCount = 0
  $effect(() => {
    const note = moveNote && moveNote.threadId === currentThreadId ? moveNote : null
    untrack(() => {
      if (note) composerNotices.show({ id: 'thread-move', cause: note.move, text: note.text, dismissLabel: 'Dismiss move note' })
      else composerNotices.withdraw('thread-move')
    })
  })

  // The sidebar keeps its scroll position while Settings takes its rows.
  let sidebarScrollTop = 0
  function keepSidebarScroll(node) {
    node.scrollTop = sidebarScrollTop
    const save = () => { sidebarScrollTop = node.scrollTop }
    node.addEventListener('scroll', save, { passive: true })
    return { destroy() { node.removeEventListener('scroll', save) } }
  }

  function selectThread(threadId, range) {
    const next = new Set(selectedThreadIds)
    const ids = shortcutThreads.map(item => item.threadId).filter(Boolean)
    const anchor = selectionAnchor ?? (selectedThreadIds.size ? null : currentThreadId)
    if (range && anchor && ids.includes(anchor)) {
      const [from, to] = [ids.indexOf(anchor), ids.indexOf(threadId)].sort((a, b) => a - b)
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

  // The rows one delete covers: the selection in sidebar order when the row is part of it, else the row alone.
  function deletionTargets(threadId) {
    if (!(selectedThreadIds.has(threadId) && selectedThreadIds.size > 1)) return [threadId]
    const order = shortcutThreads.map(item => item.threadId)
    const place = (id) => order.includes(id) ? order.indexOf(id) : order.length
    return [...selectedThreadIds].sort((left, right) => place(left) - place(right))
  }

  // The control's name, and the confirm group's spoken question.
  function deleteLabel(threadId, rowTitle, form = 'name') {
    const count = deletionTargets(threadId).length
    if (count > 1) return form === 'name' ? `Delete ${count} threads…` : `Delete ${count} threads?`
    return form === 'name' ? `Delete ${rowTitle}` : `Delete ${rowTitle}?`
  }

  // The running reply's thread stays until that reply settles.
  function deletionHoldsRun(threadId) {
    return !!runningThreadId && deletionTargets(threadId).includes(runningThreadId)
  }

  function askToDeleteThread(threadId) {
    if (deletionHoldsRun(threadId)) return
    deleteFromTitle = !!threadMenu?.fromTitle
    closeThreadMenu()
    deletingThreadIds = deletionTargets(threadId)
    deletingThreadId = threadId
  }

  function threadRowKeydown(event, threadId) {
    if (event.key === 'Delete' || event.key === 'Backspace') {
      if (threadSwitching || deletionHoldsRun(threadId)) return
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

  let dictationEscape = false
  const dictationController = createDictationController({
    invoke: (...args) => tauri.invoke(...args),
    listen: (...args) => window.__TAURI__?.event?.listen(...args),
    readDraft: () => draft,
    updateDraft: (next) => { draft = next },
    blocked: () => !!active,
    onState: (state) => {
      dictation = state.status
      holdDictationEscape(isDictationActive(state.status))
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
    onComplete: sendDictation,
    transform: (text) => cleanTranscript(text, readVoiceSettings()),
    startOptions: () => dictationStartOptions(readVoiceSettings()),
  })

  // Send when dictation stops: the finished transcript sends as Enter would.
  function sendDictation(transcript) {
    if (!transcript.trim() || !readVoiceSettings().send || active || composerActionInactive()) return
    if (needsModel()) openModelProviders()
    else chatController.send()
  }

  // Escape cancels dictation from any app while it listens, as the Voice page says.
  function holdDictationEscape(listening) {
    if (listening === dictationEscape) return
    dictationEscape = listening
    if (listening) Promise.resolve().then(() => register('Escape', ({ state }) => { if (state === 'Pressed') stopDictation(true) })).catch(() => { dictationEscape = false })
    else Promise.resolve().then(() => unregister('Escape')).catch(() => {})
  }

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
      // The user allowed large downloads before, so the download starts unasked.
      const resume = status.state === 'notInstalled' && await downloadsAllowed(tauri)
      if (destroyed) return
      speechInstallFacts = facts
      speechInstallStatus = status
      if (status.state === 'installing') {
        const epoch = ++speechInstallEpoch
        speechInstallTimer = setTimeout(() => readSpeechInstallStatus(epoch), 250)
      } else if (resume) {
        speechInstallPending = false
        void startSpeechInstall(resumeSpeechModel)
      }
    } catch (_) {
      if (destroyed) return
      speechInstallError = 'The speech model install details could not be loaded. Try again.'
    } finally {
      if (!destroyed) speechInstallPending = false
    }
  }

  async function startSpeechInstall(install = installSpeechModel) {
    if (speechInstallPending || speechInstallStatus?.state === 'installing') return
    stopSpeechInstallPolling()
    const epoch = ++speechInstallEpoch
    speechInstallPending = true
    speechInstallError = ''
    try {
      const next = await install(tauri)
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

  // A chat in the background runtime hands its browser request to the app as
  // a gate. The app works it in the Browser tab and answers each gate once.
  const browserGates = new Set()
  let browsing = null
  $effect(() => {
    for (const message of messages) {
      const run = message.run
      const gate = run?.pendingPermission
      if (run?.phase !== 'pending-permission' || gate?.kind !== 'editor' || gate.title !== 'muniment:browser') continue
      const key = `${run.id}:${gate.gateId}`
      if (browserGates.has(key)) continue
      browserGates.add(key)
      browsing = run.id
      void runBrowserGate(run, gate)
    }
    // A stopped reply stops its browse between steps.
    const running = browsing && messages.find((message) => message.run?.id === browsing)?.run
    if (running && ['cancelled', 'failed', 'interrupted'].includes(running.phase)) {
      browsing = null
      void tauri.invoke('browser_command', { request: { view: 'browser', action: 'stop', value: '' } }).catch(() => {})
    }
  })
  // The browser tool opens the Browser tab when it is closed, then works the page.
  async function openBrowserForChat() {
    showBrowser('browser')
    for (let attempt = 0; attempt < 40; attempt += 1) {
      try { return await tauri.invoke('browser_command', { request: { view: 'browser', action: 'status', value: '' } }) }
      catch (_) { await new Promise((resolve) => setTimeout(resolve, 250)) }
    }
    throw new Error('The Browser tab did not open.')
  }
  async function runBrowserGate(run, gate) {
    let value
    try {
      await openBrowserForChat()
      value = await tauri.invoke('browser_tool', { request: gate.prefill ?? '' })
    } catch (error) {
      value = JSON.stringify({ error: String(error?.message ?? error) })
    }
    browsing = null
    answerPermission(run, { type: 'editor', value })
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
    behavior: () => readVoiceSettings().behavior,
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

  let workspaceDirty = $state(false)
  let workspaceDirectory = $state('')
  $effect(() => { currentThreadId; selectedProject; workspaceDirectory = ''; untrack(closeMentions) })
  let mention = $state(null)
  let mentionFiles = $state([])
  // In a group thread, `@` lists the members and @everyone instead of files.
  let mentionAgents = $state([])
  const mentionCount = $derived(threadGroup ? mentionAgents.length : mentionFiles.length)
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
    clearTimeout(mentionTimer); mentionVersion += 1; mention = null; mentionFiles = []; mentionAgents = []; mentionLoading = false
  }
  function updateMention() {
    clearTimeout(mentionTimer)
    const version = ++mentionVersion
    // A paired runtime cannot read this computer's files, so `@` finds none there.
    mention = !active && (threadGroup || !remoteRuntime) && composer ? mentionQuery(composer.value, composer.selectionStart) : null
    mentionSelected = 0; mentionError = ''; mentionFiles = []; mentionAgents = []
    if (!mention) { mentionLoading = false; return }
    openComposerPanel('mentions')
    if (threadGroup) { mentionLoading = false; mentionAgents = agentMentions(mention.query, threadGroupMembers); return }
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
  async function chooseAgentMention(agent) {
    const current = mention
    closeMentions()
    if (!current) return
    const inserted = insertAgentMention(draft, current, agent)
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
    // The first composer that takes typing marks the page timeline. scripts/speed-timing.mjs reads the mark.
    try { if (!performance.getEntriesByName('muniment:composer-ready').length) performance.mark('muniment:composer-ready') } catch (_) {}
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
      if (action === 'sign-out') { artifactsOpen = false; projectsOpen = false; agentsOpen = false; fullPage = null }
      if (auth.name === 'signed-in') await chatController.loadHistory()
      return true
    } catch (err) {
      if (version === authRequestVersion) auth = errorState(action, err)
      return false
    }
  }

  // A connected provider that lists no model yet is read again, and the chip
  // keeps the model it named until the last read.
  // `keep` marks a re-read of the same machine, which never takes the chip's
  // model away: a runtime that just answered a reply or reconnected can list
  // none for a moment, and Settings shows a real disconnect itself.
  async function refreshInventory({ keep = false } = {}) {
    const version = ++inventoryRequestVersion
    const machine = remoteRuntime?.id ?? LOCAL_RUNTIME_ID
    try {
      await loadInventory((command) => tauri.invoke(command), (next, final) => {
        inventory = keptInventory(inventory, next, final, { keep })
        inventoryMachine = machine
        if (accountStatus === inventoryError) accountStatus = ''
        inventoryError = ''
      }, { alive: () => version === inventoryRequestVersion && !inventoryClosed, awaiting: awaitingFrom(inventory) })
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
      } catch (failure) {
        // The first line shows in Account, and the cause behind Show log.
        accountStatus = `Cloud sign-in could not start. Try again.\n${String(failure?.message ?? failure ?? '')}`.trim()
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

  // Every agent's runs on every machine, kept current from each machine's
  // `agent-run-updated` events, for agent profiles. Runs this window started
  // and runs already announced raise no system notification.
  let activityRuns = $state([])
  const startedRuns = new Set()
  const announcedRuns = new Set()
  async function refreshRuns() {
    try { activityRuns = await listRuns(tauri, runtimeList, { limit: 100 }) } catch (_) {}
  }
  async function refreshAgentsAndRuns() { await Promise.all([refreshAgents(), refreshRuns()]) }
  // Usage and Notifications take the thread's place. A second press closes them.
  function toggleFullPage(page) {
    if (settingsOpen) closeSettings()
    if (fullPage === page) { fullPage = null; return }
    projectsOpen = false; artifactsOpen = false; agentsOpen = false; agentProfileOpen = false; closeRail()
    fullPage = page
  }
  function showNotifications() { if (fullPage !== 'notifications') toggleFullPage('notifications') }
  // Settings links to Usage, the one page that shows it.
  onMount(() => {
    const showUsage = () => { if (fullPage !== 'usage') toggleFullPage('usage') }
    window.addEventListener(OPEN_USAGE_EVENT, showUsage)
    return () => window.removeEventListener(OPEN_USAGE_EVENT, showUsage)
  })
  // The Notifications badge counts what needs the user on every machine, from
  // core's page, and reads again shortly after each run or approval event.
  let needsYou = $state(0)
  let notificationsKey = $state(0)
  let notifyChoice = { needs_you: true, finished: false }
  let needsYouTimer = null
  async function refreshNeedsYou() {
    if (!tauri || !(auth.name === 'local' || auth.name === 'signed-in')) return
    try {
      const page = await readNotifications(tauri, machineRefs(runtimeList), { view: 'needs_you' })
      needsYou = page?.counts?.needs_you ?? 0
      needsYouItems = page?.needs_you ?? []
    } catch (_) {}
  }
  function noteNotifications() {
    notificationsKey += 1
    clearTimeout(needsYouTimer)
    needsYouTimer = setTimeout(() => { void refreshNeedsYou() }, 800)
  }
  const needsYouMachines = $derived(JSON.stringify(machineRefs(runtimeList)))
  $effect(() => { void needsYouMachines; void auth.name; untrack(() => { void refreshNeedsYou() }) })
  onMount(() => {
    void readNotifyPreferences(tauri).then((value) => { notifyChoice = value })
    const chosen = (event) => { notifyChoice = event.detail }
    window.addEventListener('muniment:notify-preferences', chosen)
    const poll = () => { if (!document.hidden) void refreshNeedsYou() }
    const timer = setInterval(poll, 60_000)
    return () => { clearInterval(timer); clearTimeout(needsYouTimer); window.removeEventListener('muniment:notify-preferences', chosen) }
  })
  // A run's thread takes the view: the page, catalog or agent profile that
  // covers it closes, also when it is the open thread already. A run that
  // names no thread yet is read again, since core names the thread once the
  // run starts.
  async function openRunThread(run) {
    let threadId = run?.thread_id
    if (!threadId && run?.id && run?.agent_id) {
      const runs = await listRuns(tauri, runtimeList, { agentId: run.agent_id, runtimeId: run.runtimeId ?? LOCAL_RUNTIME_ID, limit: 20 })
      threadId = runs.find((entry) => entry.id === run.id)?.thread_id
    }
    if (!threadId) throw new Error('The run has not started its thread yet. Try again in a moment.')
    return showThread(threadId)
  }
  async function showThread(threadId) {
    profileHeldFor = threadId
    if ((threadId !== currentThreadId || freshThread) && !await chatController.openThread(threadId, true)) return false
    await tick()
    fullPage = null; agentsOpen = false; projectsOpen = false; artifactsOpen = false
    agentProfileOpen = false; profileOnlyAgent = null
    return true
  }
  function agentRunUpdated(update) {
    const run = update?.run
    if (!run?.id) return
    const runtimeId = update.runtimeId || LOCAL_RUNTIME_ID
    activityRuns = applyRunUpdate(activityRuns, { runtimeId, run })
    const latest = agentListing.latestRuns[run.agent_id]
    if (!latest || latest.id === run.id || String(latest.updated_at) <= String(run.updated_at)) {
      agentListing = { ...agentListing, latestRuns: { ...agentListing.latestRuns, [run.agent_id]: { ...run, runtimeId } } }
    }
    noteNotifications()
    const placed = { ...run, runtimeId }
    if (!shouldNotify(placed, startedRuns) || announcedRuns.has(notificationKey(placed))) return
    announcedRuns.add(notificationKey(placed))
    if (!notifyAllowed(placed, notifyChoice)) return
    const agent = roster.find(entry => entry.id === run.agent_id)
    showNotification(notificationText(placed, agent?.name))
  }
  // A group's replies and a replica's runs land in the open thread after
  // core posts or replicates them, so the thread reads again.
  function swarmChanged(update) {
    const kind = update?.change?.kind
    void refreshSwarm()
    if (kind === 'agents' || kind === 'absence') void refreshAgents()
    if (kind === 'threads' && (threadGroup || threadPostTarget)) void chatController.refreshOpenThread()
    if (kind === 'threads' && pendingOffload) void chatController.refreshThreads()
  }
  function approvalChanged(update) {
    const known = swarm.approvals.some((entry) => entry.approval_id === update?.approval?.approval_id)
    swarm = { ...swarm, approvals: applyApproval(swarm.approvals, update) }
    noteNotifications()
    const approval = update?.approval
    if (approval?.state === 'pending' && !known && notifyChoice.needs_you) showNotification({ title: `${approval.agent_name || 'An agent'} needs approval`, body: approval.summary || 'Open Notifications to answer.' })
  }
  onMount(() => {
    const stops = []
    let destroyed = false
    for (const [event, handle] of [
      ['agent-run-updated', agentRunUpdated],
      ['swarm-changed', swarmChanged],
      ['agent-activity', (payload) => { liveActivity = applyActivity(liveActivity, payload) }],
      ['group-posted', (payload) => { threadPosts = applyGroupPost(threadPosts, payload) }],
      ['agent-approval-pending', approvalChanged],
      ['agent-approval-resolved', approvalChanged],
    ]) {
      void Promise.resolve(window.__TAURI__?.event?.listen(event, ({ payload }) => handle(payload))).then(unlisten => { if (destroyed) unlisten?.(); else stops.push(unlisten) })
    }
    if (tauri) stops.push(fleet.start())
    return () => { destroyed = true; for (const stop of stops) stop?.() }
  })
  // The tools, skills and MCP servers an agent profile offers: core's
  // `agent.tool_catalog` from the agent's own machine.
  let agentCatalog = $state(emptyCatalog())
  const profileAgentKey = $derived(agentProfileOpen && profileAgent && !profileAgent.replica ? `${profileAgent.runtimeId}:${profileAgent.id}` : null)
  const profileRuntime = $derived(profileAgentKey ? profileAgent.runtimeId : null)
  $effect(() => {
    const runtime = profileRuntime
    if (!runtime) return
    let current = true
    untrack(() => {
      agentCatalog = emptyCatalog()
      void loadToolCatalog(tauri, runtime).then(catalog => { if (current) agentCatalog = catalog })
      if (!activityRuns.length) void refreshRuns()
    })
    return () => { current = false }
  })

  // A hidden window skips the poll and refreshes once when it shows again.
  onMount(() => {
    const refresh = () => { if (!document.hidden && (auth.name === 'local' || auth.name === 'signed-in')) { void refreshAgents(); void refreshCreations() } }
    const agentTimer = setInterval(refresh, 15000)
    document.addEventListener('visibilitychange', refresh)
    return () => { clearInterval(agentTimer); document.removeEventListener('visibilitychange', refresh) }
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
      if (pairingRequests.some(request => request.challenge === payload?.challenge)) return
      pairingRequests = [...pairingRequests, {
        challenge: payload?.challenge,
        claimedKind: boundedAttachClaim(payload?.claimed_kind),
        claimedVersion: boundedAttachClaim(payload?.claimed_version),
        workspace: boundedAttachWorkspace(payload?.workspace),
        scopes: Array.isArray(payload?.scopes) ? payload.scopes : [],
      }]
      const challenge = payload?.challenge
      const lifetime = Number.isFinite(payload?.deadline_ms) ? Math.max(0, Math.min(payload.deadline_ms, 120000)) : 120000
      pairingTimers.set(challenge, setTimeout(() => {
        pairingRequests = pairingRequests.filter(request => request.challenge !== challenge)
        pairingTimers.delete(challenge)
      }, lifetime))
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
      // A row click leaves focus where it was, so Escape ends a selection from anywhere outside a menu or dialog.
      if (event.key === 'Escape' && selectedThreadIds.size && !event.target?.closest?.('[role="menu"], [role="dialog"]')) {
        event.preventDefault()
        clearThreadSelection()
        return
      }
      const sidebarTarget = event.target?.closest?.('#sidebar, .thread-menu')
      const editing = event.target?.closest?.('input,textarea,[contenteditable="true"]')
      if (sidebarTarget && !editing && !switchLocked) {
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
      const rowPosition = threadRowShortcutPosition(event)
      if (workspaceMode() && onboarding.name === 'complete' && rowPosition !== null) {
        event.preventDefault()
        if (sidebarCollapsed || switchLocked) return
        const threadId = shortcutThreads[rowPosition - 1]?.threadId
        if (!threadId || threadId === currentThreadId) return
        projectsOpen = false; agentsOpen = false; fullPage = null; artifactsOpen = false
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
      // A focused terminal keeps ⌘K, which clears it.
      if (workspaceMode() && onboarding.name === 'complete' && isPaletteShortcut(event) && !event.target?.closest?.('.terminal-workspace')) {
        event.preventDefault()
        paletteOpen = !paletteOpen
        return
      }
      if (featureFlags.companyRecord && workspaceMode() && onboarding.name === 'complete' && isRecordPanelShortcut(event)) {
        event.preventDefault()
        // ⌘⇧K on a maximized record panel returns the sidebar and the thread first.
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
      if (workspaceMode() && onboarding.name === 'complete' && !switchLocked && (isUsageShortcut(event) || isNotificationsShortcut(event))) {
        event.preventDefault()
        toggleFullPage(isUsageShortcut(event) ? 'usage' : 'notifications')
        return
      }
      if (event.key === 'Escape' && (pickerOpen || thinkingOpen || modeOpen || machineOpen || contextOpen || (dictation.state === 'modelNotInstalled' && !speechInstallDismissed))) { openComposerPanel(null); event.preventDefault(); return }
      if (event.key === 'Escape' && projectsOpen) { projectsOpen = false; event.preventDefault() }
      if (event.key === 'Escape' && artifactsOpen) { artifactsOpen = false; event.preventDefault() }
      if (event.key === 'Escape' && agentsOpen) { agentsOpen = false; event.preventDefault() }
      if (event.key === 'Escape' && fullPage) { fullPage = null; event.preventDefault() }
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
    // The text size shortcut listens in the capture phase, so a pane that keeps its own keys,
    // such as the terminal or the file editor, still passes the super key with =, - and 0 to it.
    const textSize = (event) => {
      if (deletingThreadId || auth.name === 'signing-in') return
      const sizeStep = typeSizeShortcutStep(event)
      if (sizeStep === null) return
      event.preventDefault()
      event.stopPropagation()
      typeSizeShortcutPressed(sizeStep)
    }
    document.addEventListener('keydown', textSize, true)
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
      composerNotices.cleanup()
      pairingUnlisten?.()
      for (const timer of pairingTimers.values()) clearTimeout(timer)
      pairingTimers.clear()
      registrationRetryUnlisten?.()
      desktopClientUnlisten?.()
      launcherUnlisten?.()
      voiceGesture.cleanup()
      holdDictationEscape(false)
      transcriptController.cleanup()
      stopDragDrop?.()
      voiceShortcutManager.cleanup()
      document.removeEventListener('keydown', shortcuts)
      document.removeEventListener('keydown', textSize, true)
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
      clearTimeout(pairingTimers.get(request.challenge))
      pairingTimers.delete(request.challenge)
      pairingRequests = pairingRequests.filter(item => item.challenge !== request.challenge)
    }
  }

  async function chooseFiles() {
    if (active) return
    submitError = ''
    let picked
    try {
      picked = await tauri.invoke('chat_pick_attachments')
    } catch (_) {
      submitError = 'Files could not be selected. Try again.'
      return
    }
    if (!picked) return
    const paths = Array.isArray(picked) ? picked : [picked]
    await addFiles(paths)
  }

  async function addFiles(paths) {
    if (remoteRuntime) {
      submitError = REMOTE_FILES_NOTE
      return
    }
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

  async function openFirstRunModelSettings(option = null, input = {}) {
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
    // SPEC.md: a found server connects in place, a subscription with an
    // account signs in from the first run, any other opens its sign-in in Models, More
    // providers opens the catalog, and explore opens the composer with no model.
    if (option?.kind === 'explore') {
      onboarding = { name: 'complete', homePath: onboarding.homePath }
      void refreshProjects()
      void tick().then(() => composer?.focus())
      return
    }
    if (option?.kind === 'server') {
      try {
        await tauri.invoke('onboarding_connect_server', { id: option.id, baseUrl: option.server.baseUrl, key: input.key ?? null, keyFromFile: !!input.keyFrom })
      } catch (error) {
        throw new Error(typeof error === 'string' && error ? error : `Muniment could not connect ${option.name}. Check the server and try again.`)
      }
      await refreshInventory()
      onboarding = { name: 'complete', homePath: onboarding.homePath }
      void refreshProjects()
      void tick().then(() => composer?.focus())
      return
    }
    const account = option?.kind === 'subscription' ? catalogProvider(option.catalog)?.account : null
    if (account) return { signIn: account }
    onboarding = { name: 'complete', homePath: onboarding.homePath }
    // Projects first loaded before Home existed, so they load again under it.
    void refreshProjects()
    settingsConnectProvider = option?.catalog ?? ''
    openSettings('providers')
  }

  // A first-run connection ends the first run on the composer with its models.
  async function finishFirstRunConnect() {
    await refreshInventory()
    onboarding = { name: 'complete', homePath: onboarding.homePath }
    void refreshProjects()
    void tick().then(() => composer?.focus())
  }

  function keydown(event) {
    if (composerExtensions?.handleKey(event)) return
    if (mention && !event.isComposing) {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); closeMentions(); return }
      if (['ArrowDown', 'ArrowUp'].includes(event.key) && mentionCount) {
        event.preventDefault()
        mentionSelected = (mentionSelected + (event.key === 'ArrowDown' ? 1 : -1) + mentionCount) % mentionCount
        void tick().then(() => document.getElementById(`${threadGroup ? 'agent' : 'file'}-mention-${mentionSelected}`)?.scrollIntoView({ block: 'nearest' }))
        return
      }
      if (event.key === 'Enter' || (event.key === 'Tab' && mentionCount)) {
        event.preventDefault()
        if (threadGroup) { if (mentionAgents[mentionSelected]) void chooseAgentMention(mentionAgents[mentionSelected]) }
        else if (mentionFiles[mentionSelected]) void chooseMention(mentionFiles[mentionSelected])
        return
      }
    }
    if (event.key === 'Tab' && event.shiftKey && !event.isComposing && composerModeShown) {
      event.preventDefault()
      cycleMode()
      return
    }
    const action = composerAction(event, outgoingDraft(), active)
    if (action) {
      event.preventDefault()
      if (action === 'submit' && needsModel()) openModelProviders()
      else action === 'submit' ? sendOrRun() : chatController.queue('steer')
    }
  }

  // With no model connected, the composer's Send and model chip open the
  // provider catalog in Settings instead of a run that cannot start.
  function needsModel() {
    return !agentChat && auth.name === 'local' && !!inventory && !chipModel
  }

  function openModelProviders() {
    settingsConnectProvider = ''
    openSettings('providers', modelChip)
  }

  // The runtime rejects a new run while its upgrade is pending. The
  // shell holds send, resume and queue until a compatible welcome clears the flag.
  function runtimeUpgradePending() {
    return desktopClientStatus?.runtime_upgrade_pending === true
  }

  // A run left running on another thread holds a send only on its own
  // machine. A post or a new thread on a fleet member starts no run here.
  function backgroundHolds() {
    if (!backgroundRun || threadPostTarget || (freshThread && offloadTarget)) return false
    return (backgroundRun.runtime ?? LOCAL_RUNTIME_ID) === composerRuntime
  }

  function sendDisabled() {
    if (agentChat) return !!agentChatPending || !draft.trim()
    return runtimeUpgradePending() || !!unreachableMachine || !outgoingDraft().trim() || backgroundHolds() || active?.phase === 'resuming' || active?.id === 'pending' || (!active && (dictationBusy() || threadSwitching))
  }

  // The band's one action control: an up arrow that sends while the draft has
  // text, and a stop square while a reply is in flight. Enter steers in flight.
  function composerActionInactive() {
    return active ? active.id === 'pending' || active.phase === 'resuming' : sendDisabled() || modelChoicePending || !!retryReview?.chooseModel
  }

  function composerActionClick() {
    if (composerActionInactive()) return
    if (active) chatController.cancel()
    else if (needsModel()) openModelProviders()
    else sendOrRun()
  }
</script>

<!-- Agents and Artifacts sit over Projects as quiet rows. Each opens its dashboard, Artifacts with its ⌘J shortcut. -->
{#snippet libraryRow()}
  <button type="button" class="side-action library-entry" aria-label="Agents" aria-expanded={agentsOpen} aria-current={agentsOpen ? 'true' : undefined} disabled={switchLocked} onclick={() => { if (agentsOpen) agentsOpen = false; else void showAgents() }}><LucideIcon name="bot" /><span>Agents</span></button>
  <button type="button" class="side-action library-entry" aria-label="Artifacts" aria-expanded={artifactsOpen} aria-current={artifactsOpen ? 'true' : undefined} aria-keyshortcuts={artifactShortcut} disabled={switchLocked} onclick={toggleArtifactRail}><LucideIcon name="file-code" /><span>Artifacts</span><kbd>{shortcutDisplayLabel(artifactShortcut)}</kbd></button>
{/snippet}

{#snippet freshRow()}
  {#if showFreshThread}
    {@const projectName = selectedProject ? sidebarProjectById[selectedProject]?.name : ''}
    <li class="thread-row active-thread" data-fresh-thread aria-current="true" aria-keyshortcuts={freshThreadPosition >= 0 && freshThreadPosition < 9 ? threadRowShortcut(freshThreadPosition + 1) : undefined} data-accent-edge><div class="thread-row-body"><div class="thread-row-title">{currentThreadTitle}</div><span class="thread-row-meta"><StatusChip tone="neutral">New</StatusChip>{#if projectName}<span class="row-project">{projectName}</span>{/if}</span></div></li>
  {/if}
{/snippet}

<!-- A row core arranged: the thread's summary when the window holds one, with core's row beside it. -->
{#snippet coreRow(row)}
  {@render sidebarThread({ ...(threadSummaries.find((summary) => summary.threadId === row.thread_id) ?? { threadId: row.thread_id, title: row.title, updatedAt: row.updated_at }), row })}
{/snippet}

{#snippet machineBadge(summary)}
  {@const machine = summary.runtimeId && summary.runtimeId !== LOCAL_RUNTIME_ID ? runtimeList?.runtimes?.find((runtime) => runtime.id === summary.runtimeId) : null}
  {#if machine}<span class="machine-badge"><i class="machine-dot" data-tone={machineStatusTone(machine.status)} aria-hidden="true"></i><span class="machine-name">{machineName(machine)}</span><span class="visually-hidden">, {statusLabel(machine)}</span></span>{/if}
{/snippet}

<!-- A row's meta line: its status in words and its project, then for an
     agent's thread the Agents icon and the agent's name, or once a machine is
     paired the machine the thread lives on, by its shape and name. At the
     right end, how long its run has been working or how long ago the thread
     last changed. A settled thread shows only its time. -->
{#snippet rowMeta(summary, id = undefined)}
  {@const agent = summary.row?.agent}
  {@const owner = !agent && threadOwner(summary, swarm, runtimeList)}
  {@const machine = !agent && !owner && pairedRuntimes(runtimeList).length ? runtimeList.runtimes.find((runtime) => runtime.id === (summary.runtimeId ?? LOCAL_RUNTIME_ID)) : null}
  {@const at = (summary.row?.status === 'working' && sidebarRuns.since.get(summary.threadId)) || summary.updatedAt}
  <span class="thread-row-meta" {id}>{#if summary.row?.status_label}<StatusChip tone={statusTone(summary.row.status)}>{summary.row.status_label}</StatusChip>{#if summary.row.project_name}<span class="row-project">{summary.row.project_name}</span>{/if}<span aria-hidden="true">·</span>{/if}{#if agent}<LucideIcon name="bot" size={12} /><span class="row-agent">{agent.name}</span>{:else if owner}<OwnerChip machine={owner} />{:else if machine}<LucideIcon name={kindIcon(machine)} size={12} /><span class="machine-name">{machineName(machine)}</span>{#if statusHint(machine)}<StatusChip dot tone={machineStatusTone(machine.status)}>, {statusLabel(machine)}</StatusChip>{/if}{/if}<time datetime={at}>{relativeTime(at)}</time></span>
{/snippet}

{#snippet unconnectedModelIcon()}<LucideIcon name="cpu" size={14} />{/snippet}

<!-- While rows are selected, each row leads with its check box. Otherwise a row leads with its text. -->
{#snippet rowMark(selected)}
  {#if selectedThreadIds.size}<span class="select-mark" aria-hidden="true"><LucideIcon name={selected ? 'square-check' : 'square'} size={14} variant="action" /></span>{/if}
{/snippet}

{#snippet sidebarThread(summary)}
                {@const creation = creations.find((item) => item.threadId === summary.threadId)}
                {@const title = creation?.kind === 'agent' && creation.turns?.length ? agentChatTitle(creation) : creation ? creationTitle(creation, [summary]) : summary.title || summary.row?.title || 'New thread'}
                {@const current = (!freshThread || listedThreadId === summary.threadId) && summary.threadId === (currentThreadId ?? threadSummaries[0]?.threadId)}
                {@const rowTitle = current && !creation ? summary.title || currentThreadTitle : title}
                {@const rowPosition = shortcutThreads.findIndex((item) => item.threadId === summary.threadId) + 1}
                {@const selected = selectedThreadIds.has(summary.threadId)}
                {@const rowId = nextRowId()}
                <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                <li class="thread-record" class:selected draggable={threadMovable(summary.threadId) && rowRename?.threadId !== summary.threadId ? 'true' : undefined} ondragstart={(event) => threadDragStart(event, summary.threadId)} oncontextmenu={(event) => openThreadMenu(event, summary.threadId)}>
                  {#if rowRename?.threadId === summary.threadId}
                    <form class="row-rename" onsubmit={(event) => { event.preventDefault(); void saveSidebarName() }}>
                      <input aria-label="Thread name" maxlength="80" bind:this={rowRenameInput} bind:value={rowRename.title} onkeydown={sidebarNameKeydown} disabled={rowRenamePending} />
                      <button type="submit" disabled={rowRenamePending || !rowRename.title.trim()}>Save</button>
                      <button type="button" disabled={rowRenamePending} onclick={() => { rowRename = null; focusThreadRow(summary.threadId) }}>Cancel</button>
                    </form>
                  {:else if current && !catalogTitle}
                    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
                    <div class="thread-row active-thread" tabindex="-1" class:selected data-thread-id={summary.threadId} data-accent-edge style:--edge-color={summary.row?.agent?.color} aria-current="true" aria-keyshortcuts={rowPosition > 0 && rowPosition <= 9 ? threadRowShortcut(rowPosition) : undefined} onclick={(event) => currentRowClick(event, summary.threadId)}>{@render rowMark(selected)}<div class="thread-row-body"><div class="thread-row-title">{rowTitle}</div>{@render rowMeta(summary)}</div></div>
                  {:else}
                    {@const running = summary.threadId === backgroundRun?.threadId}
                    <button class="thread-row" class:selected aria-labelledby={`${rowId}-title`} aria-describedby={`${rowId}-meta`} data-thread-id={summary.threadId} data-selected={selected ? 'true' : undefined} data-running={running ? 'true' : undefined} aria-current={current ? 'true' : undefined} aria-keyshortcuts={rowPosition > 0 && rowPosition <= 9 ? threadRowShortcut(rowPosition) : undefined} aria-disabled={runLocked ? 'true' : undefined} onclick={(event) => sidebarRowClick(event, summary)} onkeydown={(event) => threadRowKeydown(event, summary.threadId)}>{@render rowMark(selected)}<div class="thread-row-body"><div class="thread-row-title" id={`${rowId}-title`}>{rowTitle}</div>{@render rowMeta(summary, `${rowId}-meta`)}</div>{#if running}<span class="visually-hidden">Running</span>{/if}</button>
                  {/if}
                  {#if deletingThreadId !== summary.threadId && rowRename?.threadId !== summary.threadId}
                    <button type="button" class="quiet thread-actions" aria-label={`Actions for ${rowTitle}`} aria-haspopup="menu" aria-expanded={threadMenu?.threadId === summary.threadId} onclick={(event) => openThreadMenu(event, summary.threadId)}><LucideIcon name="ellipsis-vertical" variant="action" size={14} /></button>
                  {/if}

                </li>
{/snippet}

<main class:onboarding-active={tauri && onboarding.name !== 'complete'}>
  {#if !workspaceMode() || onboarding.name !== 'complete'}
    <div class="lockup">
      <PocketFold size={onboarding.name === 'complete' ? 96 : 76} />
      {#if onboarding.name === 'complete' && (auth.name === 'signed-out' || auth.name === 'signing-in')}
        <h1 class="name">muniment</h1>
      {:else}
        <span class="name">muniment</span>
      {/if}
    </div>
    {#if onboarding.name === 'complete'}<p class="meta">shell v{version}</p>{/if}
  {/if}

  {#if tauri}
    <Onboarding {tauri} bind:onboarding onready={openFirstRunModelSettings} onsignedin={finishFirstRunConnect} />
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
      <section class="workspace" inert={auth.name === 'signing-in' || !!deletingThreadId} data-testid={auth.name === 'local' ? 'local-mode' : undefined} class:macos={macOS} class:sidebar-collapsed={sidebarCollapsed} class:rail-open={railOccupant !== null} class:agents-open={agentsOpen || artifactsOpen || projectsOpen || !!fullPage} class:tools-open={!!browserPanel} class:agent-profile-open={agentProfileOpen && !!profileAgent && !agentsOpen} class:record-maximized={(recordPanelOpen || filePanelOpen) && recordMaximized} class:artifact-resizing={artifactRailPointer !== undefined || workspacePanelPointer !== undefined} class:sidebar-resizing={sidebarPointer !== undefined} class:settings-open={settingsOpen} style:--workspace-panel-width={`${workspacePanelWidth}px`} style:--agent-panel-width={`${agentPanelWidth}px`} style:--artifact-rail-width={`${artifactRailWidth}px`} style:--sidebar-column={`${sidebarCollapsed ? 0 : sidebarWidth}px`} bind:this={workspace}>
        <header class="titlebar" data-tauri-drag-region use:overflowLabels={'.agent-chat-title span'}>
          <div class="titlebar-sidebar" data-tauri-drag-region>
            <button type="button" class="quiet side-toggle" aria-controls="sidebar" aria-expanded={!sidebarCollapsed} aria-keyshortcuts={sidebarKeyShortcut} aria-label={`${sidebarCollapsed ? 'Expand' : 'Collapse'} sidebar`} onclick={toggleSidebar}>
              <LucideIcon name={sidebarCollapsed ? 'panel-left-open' : 'panel-left-close'} />
            </button>
          </div>
          <div class="titlebar-thread" data-tauri-drag-region>
            {#if catalogTitle}<span class="catalog-title" data-tauri-drag-region>{catalogTitle}</span>
            {:else}
            {#if threadAgent}<button class="quiet agent-chat-title" data-accent-edge style:--agent-color={agentColor(threadAgent)} style:--edge-color={agentColor(threadAgent)} aria-label={`Open profile for ${threadAgent.name}`} onclick={revealAgentProfile}><AgentAvatar agent={threadAgent} activity={threadAgent.activity} size={20} /><span>{threadAgent.name}</span></button>{/if}
            {#if !threadAgent}
            {#if editingThreadTitle}
              <input class="thread-title" aria-label="Thread name" maxlength="160" bind:this={threadTitleInput} value={threadTitleDraft} oninput={limitThreadTitle} onkeydown={threadTitleKeydown} onblur={commitThreadTitle}>
            {:else}
              <h1 class="thread-title-heading" aria-label={currentThreadTitle} data-tauri-drag-region><RowControl kind="thread-title" data-accent-edge aria-label="Rename thread" disabled={!currentThreadId || freshThread || !!active || !!agentChat} bind:element={threadTitleButton} onclick={() => editThreadTitle(currentThreadTitle)} onkeydown={threadTitleButtonKeydown}>{currentThreadTitle}</RowControl></h1>
              {#if threadMachine && threadMachine !== LOCAL_RUNTIME_ID}{@render machineBadge({ runtimeId: threadMachine })}{/if}
            {/if}
            <button type="button" class="quiet title-thread-actions" aria-label="Thread actions" aria-haspopup="menu" aria-expanded={!!threadMenu?.fromTitle} disabled={!currentThreadId || freshThread || threadSwitching} bind:this={titleActionsButton} onclick={(event) => openThreadMenu(event, currentThreadId, true)}><LucideIcon name="ellipsis-vertical" variant="action" size={14} /></button>
            {/if}
            {/if}

            <span class="title-spacer" data-tauri-drag-region></span>
            <span class="update-slot"><AppUpdate {tauri} busy={workspaceDirty || threadSwitching || isDictationActive(dictation) || dictationFinishing} /></span>
            {#if featureFlags.companyRecord}<RowControl kind="record-toggle" aria-controls="record-panel" aria-expanded={recordPanelOpen} aria-keyshortcuts={recordShortcut} aria-label={`${recordPanelOpen ? 'Close' : 'Open'} record panel`} onclick={toggleRecordPanel}>Record <kbd>{shortcutDisplayLabel(recordShortcut)}</kbd></RowControl>{/if}
            <WorkspaceMenu selected={browserPanel} onselect={showBrowser} onopenchange={value => toolsMenuOpen = value} />
          </div>
        </header>
                  {#if threadMenu}
                    {@const summary = threadSummaries.find((item) => item.threadId === threadMenu.threadId) || { threadId: threadMenu.threadId, title: currentThreadTitle }}
                    {@const rowTitle = summary.title || 'New thread'}
                    {@const running = summary.threadId === runningThreadId}
                    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                    <div class="thread-menu" use:floatingMenu role="menu" tabindex="-1" aria-label={`${rowTitle} actions`} style:left={`${threadMenu.x}px`} style:top={`${threadMenu.y}px`} onkeydown={threadMenuKeydown} onfocusout={menuFocusOut} use:focusMenuOnMount>
                      <button type="button" role="menuitem" disabled={running} onclick={() => { if (threadMenu.fromTitle) { closeThreadMenu(); editThreadTitle(currentThreadTitle) } else renameSidebarThread(summary) }}>Rename</button>
                      {#if !threadMenu.fromTitle && !selectedThreadIds.has(summary.threadId)}<button type="button" role="menuitem" onclick={() => { const id = summary.threadId; closeThreadMenu(); selectThread(id, false); focusThreadRow(id) }}>Select</button>{/if}
                      <button type="button" role="menuitem" onclick={() => setThreadOrganization(summary.threadId, 'pinned')}>{threadOrganization[summary.threadId]?.pinned ? 'Unpin' : 'Pin'}</button>
                      {#if !agentThreadMap[summary.threadId] && !swarm.groups.some((group) => group.thread_id === summary.threadId)}<button type="button" role="menuitem" disabled={running} onclick={() => openMoveDialog(summary.threadId)}>Move to project</button>{/if}
                      <button type="button" role="menuitem" disabled={running} onclick={() => setThreadOrganization(summary.threadId, 'archived')}>{threadOrganization[summary.threadId]?.archived ? 'Restore' : 'Archive'}</button>
                      {#if !agentThreadMap[summary.threadId] && !agentChatRows.some((chat) => chat.threadId === summary.threadId)}<button type="button" role="menuitem" disabled={switchLocked} onclick={() => { const { threadId, title } = summary; closeThreadMenu(); void openAgentChat({ fromThread: { id: threadId, title: title || rowTitle }, projectId: projectCatalog.projects[projectCatalog.threads[threadId]] ? projectCatalog.threads[threadId] : null }) }}>Make this an agent</button>{/if}
                      <button type="button" role="menuitem" aria-label={deleteLabel(summary.threadId, rowTitle)} disabled={threadSwitching || runLocked || deletionHoldsRun(summary.threadId)} onclick={() => askToDeleteThread(summary.threadId)}>{deletionTargets(summary.threadId).length > 1 ? deleteLabel(summary.threadId, rowTitle) : 'Delete'}</button>
                    </div>
                  {/if}
        <aside data-panel="sidebar" id="sidebar" class="sidebar" use:overflowLabels={'.side-action span, .thread-row-title, .project-title span'}>
          {#if !sidebarCollapsed && settingsOpen}
            <!-- Settings lists its sections in this same sidebar, in its rows and spacing. -->
            <div class="side-top">
              <button class="side-action settings-back" aria-keyshortcuts="Escape" onclick={closeSettings}><LucideIcon name="arrow-left" /><span>Back to threads</span></button>
            </div>
            <!-- One search over every page and part. The platform's find shortcut focuses it. -->
            <div class="settings-search">
              <LucideIcon name="search" size={14} />
              <input id="settings-search" type="search" data-focus-frame placeholder="Search settings" aria-label="Search settings" aria-keyshortcuts={isMacPlatform ? 'Meta+F' : 'Control+F'} autocomplete="off" spellcheck="false" bind:value={settingsQuery}>
            </div>
            <nav class="side-scroll" aria-label="Settings sections" use:panelScroll>
              {#each settingsNav(settingsQuery, { remote: !!remoteRuntime }) as [id, label, icon, parts] (id)}
                <button class="side-action" aria-current={settingsPage(settingsSection) === id ? 'true' : undefined} onclick={() => { settingsSection = id }}><LucideIcon name={icon} /><span>{label}</span></button>
                {#each parts as [part, name] (part)}
                  <button class="side-action settings-result" onclick={() => { settingsSection = part }}><span>{name}</span></button>
                {/each}
              {:else}
                <p class="settings-empty" role="status">No setting matches.</p>
              {/each}
            </nav>
          {:else if !sidebarCollapsed}
            <!-- New and Search lead the sidebar. A selection takes the search's row, so no row moves under the pointer. -->
            <div class="side-top sidebar-head">
              <button class="side-action new-thread" aria-label="New thread" aria-keyshortcuts={newThreadKeyShortcut} disabled={switchLocked} onclick={() => newSidebarThread()}>
                <LucideIcon name="square-pen" /><span>New</span>
              </button>
              {#if selectedThreadIds.size}<div class="selection-bar"><span role="status">{selectedThreadIds.size} selected</span><button type="button" disabled={switchLocked || [...selectedThreadIds].includes(runningThreadId)} onclick={() => askToDeleteThread([...selectedThreadIds][0])}>Delete…</button><button type="button" onclick={clearThreadSelection}>Cancel</button></div>
              {:else}<div class="thread-search-field"><LucideIcon name="search" size={14} /><input class="thread-search" type="search" autocomplete="off" aria-label="Search threads" placeholder="Search" bind:this={threadSearchInput} bind:value={threadSearch} oninput={filterThreads} />{#if !threadSearch}<button type="button" class="palette-open" aria-label="Open the command palette" aria-keyshortcuts={searchKeyShortcut} onclick={() => { paletteOpen = true }}><kbd>{shortcutDisplayLabel(searchKeyShortcut)}</kbd></button>{/if}</div>{/if}
            </div>
            <div class="side-scroll" use:panelScroll use:keepSidebarScroll>
            {#if folderAccess}<FolderAccessPrompt access={folderAccess} onallow={() => folderAccessWatch?.allow()} />{/if}
            {#if swarm.absence.paused}<AbsencePrompt {tauri} runtime={swarm.self[LOCAL_RUNTIME_ID] ? LOCAL_RUNTIME_ID : Object.keys(swarm.self)[0]} pausedAt={swarm.absence.paused_at} count={roster.filter(agent => agent.paused_reason === 'absence').length} onresumed={() => refreshAgents()} />{/if}
            <SidebarSections sidebar={sidebarArrangement} row={coreRow} fresh={freshRow} library={libraryRow} query={threadSearch} pages={sidebarPages} owners={projectOwners}
              showFresh={showFreshThread} disabled={switchLocked} expanded={expandedProjects} bind:archivedOpen
              {projectForm} bind:projectName {projectBusy} {projectError}
              onpage={openSidebarPage} ontoggleproject={selectProject} onnewthread={newProjectThread} onsettings={showProjectSettings} dragType={THREAD_DRAG} onmovethread={(threadId, projectId) => { void moveThread(threadId, projectId) }}
              onrenameproject={(id, name) => { projectForm = id; projectName = name }} onopenfolder={openProjectFolder}
              ongroup={roster.length > 1 ? () => { groupCreateOpen = true } : undefined}
              onnewproject={() => { projectForm = 'new'; projectName = ''; projectsOpen = false }} onsaveproject={saveProject} oncancelproject={() => { projectForm = null }}
              onretryprojects={() => refreshProjects()}>
              {#snippet footer()}
                {#if threadSearch.trim() && !sidebarArrangement.results.length}<p class="side-empty">{loadingOlderThreads ? 'Searching threads…' : 'No matching threads.'}</p>{/if}
                {#if !threadSearch.trim() && !sidebarArrangement.projects.length && !showFreshThread}<p class="side-empty">No threads yet.</p>{/if}
                {#if threadListError}<p class="side-empty" role="alert">{threadListError} {#if threadListErrorAction}<button disabled={loadingOlderThreads} onclick={loadOlderThreads}>Retry</button>{/if}</p>{/if}
                {#if moreThreads && !threadListError}
                  <button type="button" class="older-threads" disabled={loadingOlderThreads} onclick={loadOlderThreads}>Older threads</button>
                {/if}
              {/snippet}
            </SidebarSections>
            </div>
          {/if}
          <!-- The foot stays on both sides: Settings shows as the current page there, and a second press goes back. -->
          {#if !sidebarCollapsed}
          <div class="side-foot">
            <div class="settings-block">
              <SidebarActions bind:settingsButton {settingsOpen} usageOpen={fullPage === 'usage'} notificationsOpen={fullPage === 'notifications'} {needsYou} settingsShortcut={settingsKeyShortcut} usageShortcut={usageShortcut()} notificationsShortcut={notificationsShortcut()} disabled={switchLocked} onsettings={toggleSettings} onusage={() => toggleFullPage('usage')} onnotifications={() => toggleFullPage('notifications')} />
            </div>
            {#if featureFlags.cloud}
            {#if auth.name === 'signed-in'}
              <AccessPanel {tauri} subject={auth.subject} onSignOut={() => run('sign-out')} />
            {:else}
              <button class="side-action" disabled={!!active || !!backgroundRun || localEntryPending} onclick={signIn}><LucideIcon name="log-in" size={18} /><span>Sign in to cloud</span></button>
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
          ><span class="divider-grip" aria-hidden="true"><LucideIcon name="ellipsis-vertical" size={14} variant="action" /></span></div>
        {/if}
        <div data-panel="chat" class="thread-panel" style:--composer-height="{composerBoxHeight}px" style:--file-chip-height={changed.length ? '40px' : '0px'}>
          {#if profileAgent && !agentProfileOpen && !agentsOpen}<button type="button" data-panel-control="corner" class="quiet" aria-label="Reveal agent profile" onclick={revealAgentProfile}><LucideIcon name="panel-right-open" /></button>{/if}
        {#if draggingFiles}<div class="drop-affordance" role="status"><strong>Drop files to add them</strong><span>Saved locally · supported images sent with first prompt</span></div>{/if}
        <div class="thread-shell" data-panel-fade="chat">
        <div class="thread" role="region" aria-label={`Transcript: ${currentThreadTitle}`} bind:this={thread} onscroll={transcriptScrolled}>
          {#if historyError}<p class="history-error" role="alert">{historyError} {#if historyErrorAction}<button onclick={historyErrorAction.run}>{historyErrorAction.label}</button>{/if}</p>{/if}
          {#if agentChat}
            <AgentDraftChat {tauri} turns={agentChat.record?.turns ?? []} pending={agentChatPending?.threadId === agentChat.threadId ? agentChatPending.message : ''} error={agentChatError} agentId={agentChat.record?.resultId ?? null}
              projectId={projectCatalog.projects[projectCatalog.threads[agentChat.threadId]] ? projectCatalog.threads[agentChat.threadId] : null} model={agentChatModel} runs={activityRuns}
              onsend={(text) => sendAgentChat(text)} onsaved={(agent) => agentChatSaved(agent, agentChat.threadId)}
              onopenchat={(agent, run) => (run ? openRunThread(run) : openBuiltAgent(agent)).catch(error => { submitError = String(error) })} />
          {/if}
          {#if messages.length === 0 && currentPosts.length === 0 && !currentCreation && !agentChat}{#if !threadGroup && !threadAgent && !(auth.name === 'local' && (inventoryError || !inventory || !chipModel || !currentModelAvailable(inventory)))}<ThreadStarters disabled={switchLocked || !!active} onpick={pickThreadStarter} />{:else}<p class="empty">{#if threadGroup}Write to the group. Mention agents with @name or @everyone.{:else}{auth.name === 'local' && inventoryError ? inventoryError : auth.name === 'local' && !inventory ? 'Checking available models…' : auth.name === 'local' && !chipModel ? 'Connect a model in the composer to start chatting.' : auth.name === 'local' && !currentModelAvailable(inventory) ? 'This model needs an account. Connect one or choose another.' : `Chat with ${threadAgent?.name} using ${modelSourceLabel}.`}{/if}</p>{/if}{/if}
          {#if currentCreation && !agentChat}
            {#if !pendingCreation}<details class="creation-goal" aria-label="Creation goal"><summary><strong>{creationName(currentCreation.kind, currentCreation)}</strong> <span>{currentCreation.goal}</span></summary><p>Output: {currentCreation.output}</p></details>{/if}
            {#if !messages.length && pendingCreation && creationStarters[currentCreation.kind].concat(creationDefaults(currentCreation.kind)).some(option=>option.goal===currentCreation.goal)}<div class="creation-suggestions" aria-label="Creation suggestions">{#each creationStarters[currentCreation.kind] as option (option.id)}<button aria-pressed={option.goal===currentCreation.goal} onclick={()=>useCreationSuggestion(option)}>{option.name}</button>{/each}</div>{/if}
          {/if}
          <i class="transcript-edge" hidden></i>
          {#each shownItems as item (item.post ? `post:${item.post.message_id}` : messageKeys.get(item.message))}
            {#if item.post}
              <ThreadPost post={item.post} agents={roster} section={postSections.get(item.post.message_id)} waiting={!threadGroup} />
            {:else}
            {@const message = item.message}
            {@const envelope = message.role === 'user' ? agentEnvelope(message.text) : null}
            {#if envelope}
              <div class="agent-turn"><AgentMessage {envelope} agent={roster.find((agent) => agent.id === envelope.agentId) ?? null} sentAt={message.sentAt ?? ''} /></div>
            {:else if message.role === 'user'}
                {@const userCopyId = `user:${message.id ?? message.submissionId}`}
                {@const sent = promptFolders(message.text)}
                {@const created = sentCreation(sent.text)}
              <div class="user-turn">
                <div class="user-message">
                  {#if created}<details class="creation-request"><summary><strong>{created.name}</strong> <span>{created.goal}</span></summary><p>{sent.text}</p></details>{#if created.details}<p>{created.details}</p>{/if}
                  {:else if sent.text}<p>{sent.text}</p>{:else}<p class="missing-prompt">Prompt unavailable</p>{/if}
                  {#if sent.folders.length}
                    <ul class="message-attachments" aria-label="Attached folders">
                      {#each sent.folders as folder}
                        <li><span>{folder.split(/[\\/]/).filter(Boolean).at(-1) ?? folder}</span><strong>{folder}</strong></li>
                      {/each}
                    </ul>
                  {/if}
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
                  {#if userTurnRuns.get(message) && !threadGroup}{@const turnRun = userTurnRuns.get(message)}<button type="button" aria-label={rewindBlocked ? `Rewind to here. ${rewindBlocked}` : 'Rewind to here'} data-tooltip={rewindBlocked ?? 'Rewind to here'} aria-disabled={!!rewindBlocked} class="rewind-turn" onclick={() => { if (!rewindBlocked) rewinding = { runId: turnRun, prompt: sent.text } }}></button>{/if}
                  <button type="button" aria-label={copyConfirmed(copy, userCopyId) ? 'Copied message' : 'Copy message'} data-tooltip={copyConfirmed(copy, userCopyId) ? 'Copied' : 'Copy message'} onclick={() => copyResponse({ id: userCopyId, text: sent.text })}><LucideIcon name={copyConfirmed(copy, userCopyId) ? 'check' : 'copy'} variant="action" size={14} /></button>
                </div>
                {#if copyFailure(copy, userCopyId, modifierLabel)}<p class="copy-failure">{copyFailure(copy, userCopyId, modifierLabel)}</p>{/if}
              </div>
            {:else if silentRun(message.run)}
              <p class="silent-note" role="note">Silent run. Nothing needed a reply.</p>
            {:else}
            {@const parts = responseParts(message.run)}
            <div class="response">
              {#if settledPhases.has(message.run.phase)}<RunLine run={message.run} machine={runMachine} modes={modeListing?.listing?.modes ?? []} sources={message.run.id === newestRunId ? runSources(message.run) : {}} onreview={message.run.phase === 'complete' && !threadGroup && !currentSummary?.replica ? () => { reviewing = { runId: message.run.id } } : null} />{/if}
              {#if message.run.promptStorageNotice}
                <details class="prompt-storage-notice">
                  <summary>Prompt text stays in runtime memory for this run.</summary>
                  <p>{message.run.promptStorageNotice}</p>
                </details>
              {/if}
              {#if message.run.resumeNotice}
                <p class="resume-notice"><span data-tooltip={message.run.resumeNotice} tabindex="0">Resumed after update</span></p>
              {/if}
              {#if message.run.phase === 'acquiring-pi'}
                <p class="thinking">{runAnnouncement(message.run)}</p>
              {/if}
              {#each parts as part, index}
                {#if part.type === 'actions'}
                  <ActionFeedback onopenfile={openFile} loadoutput={(effectId) => runDetail(message.run.id, effectId, 'tool_output')} activities={part.activities} live={['thinking', 'streaming', 'pending-permission'].includes(message.run.phase)} />
                {:else if message.run.phase === 'streaming'}
                  <div class="streaming"><AssistantMarkdown {tauri} onopenlink={openChatLink} text={part.text} caret={index === parts.length - 1} /></div>
                {:else}<AssistantMarkdown {tauri} onopenlink={openChatLink} text={part.text} />{/if}
              {/each}
              {#if message.run.phase === 'recovering'}<p class="thinking">Restoring reply…</p>{/if}
              {#each message.run.appliedDiffs ?? [] as appliedDiff}
                {#if appliedDiff.summary}
                  {@const request = appliedDiffReads[`${message.run.id}\n${appliedDiff.effectId}`]}
                  <details class="applied-diff tool-card" ontoggle={(event) => { if (event.currentTarget.open) readAppliedDiff(message.run.id, appliedDiff.effectId) }}>
                    <summary><strong>Applied file changes</strong><span>{appliedDiffLabel(appliedDiff.summary)}</span></summary>
                    {#if request}
                      {#await request}<p>Loading the changes.</p>
                      {:then diff}{#await loadCodeDiff() then module}{@const CodeDiff = module.default}<CodeDiff codeDiff={diff} />{/await}
                      {:catch}<p role="alert">The changes could not be loaded. <button type="button" onclick={() => readAppliedDiff(message.run.id, appliedDiff.effectId, true)}>Load changes</button></p>
                      {/await}
                    {/if}
                  </details>
                {:else}
                  <div class="applied-diff tool-card">
                    <strong>Applied file changes</strong>
                    <p>The changes were applied, but their record is no longer stored.</p>
                  </div>
                {/if}
              {/each}
              {#if message.run.phase === 'failed'}<div class="run-error">{runFailureMessage(message.run)} <button disabled={!message.run.prompt?.trim() || !!active || (!!draft.trim() && draft !== message.run.prompt) || !!selectedFiles.length || dictationBusy() || runtimeUpgradePending()} onclick={() => prepareRetry(message.run)}>Try again</button>{#if runFailureDetails(message.run).length}<details class="run-error-details"><summary>Show log</summary>{#each runFailureDetails(message.run) as line, index (index)}<p>{line}</p>{/each}</details>{/if}</div>{/if}
              {#if message.run.phase === 'cancelled'}<div class="run-error">Reply stopped. {#if message.run.prompt}<button disabled={!!active || (!!draft.trim() && draft !== message.run.prompt) || !!selectedFiles.length || dictationBusy() || runtimeUpgradePending()} onclick={() => prepareRetry(message.run)}>Try again</button>{/if}</div>{/if}
              {#if message.run.phase === 'interrupted'}<div class="run-error" role={message.run.resumeError ? 'alert' : undefined}>{message.run.resumeError ?? 'Reply interrupted.'} {#if message.run.resumable}<button disabled={!!active || !!backgroundRun || dictationBusy() || runtimeUpgradePending()} onclick={() => chatController.resume(message.run)}>Resume</button>{:else if message.run.prompt}<button disabled={!!active || (!!draft.trim() && draft !== message.run.prompt) || !!selectedFiles.length || dictationBusy() || runtimeUpgradePending()} onclick={() => prepareRetry(message.run)}>Try again</button>{/if}</div>{/if}
              {#if message.run.phase === 'pending-permission' && message.run.pendingPermission && !openApprovals.some(approval => approval.run_id === message.run.id)}
                {@const gate = message.run.pendingPermission}
                {@const answerState = permissionState(message.run)}
                {#if gate.kind === 'editor' && gate.title === 'muniment:ask_user_question'}
                  {#key gate.gateId}<UserQuestion anchor={composerBox} payload={gate.prefill} pending={answerState?.pending ?? false} error={answerState?.error ?? ''} onanswer={(value) => answerPermission(message.run, { type: 'editor', value })} />{/key}
                {:else if gate.kind === 'editor' && gate.title === 'muniment:browser'}
                  <p class="run-status" role="status">Using the browser…</p>
                {:else}
                {@const choices = composerGateOptions(gate, !!threadAgent)}
                <div class="permission-card tool-card">
                  {#if gate.presentation && !isPlanGate(gate)}<ToolCall variant="card" presentation={gate.presentation} reason={gate.reason ?? ''} input={ruleInput(gate.rule)} />
                  {:else}<strong>{gate.kind === 'code_diff' ? 'Proposed file changes' : gate.title}</strong>{/if}
                  {#if isPlanGate(gate) && composerPlan?.gateId === gate.gateId && composerPlan.text}<div class="permission-plan" role="region" aria-label="Plan">{composerPlan.text}</div>{/if}
                  {#if gate.kind === 'confirm' && gate.message}<p>{gate.message}</p>{/if}
                  {#if gate.kind === 'code_diff' && gate.diff}{#await loadCodeDiff() then module}{@const CodeDiff = module.default}<CodeDiff codeDiff={gate.diff} />{/await}
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
                    <button disabled={answerState?.pending} onclick={() => answerPermission(message.run, gate.kind === 'select' ? choices.deny : { type: 'cancelled' })}>Deny</button>
                    <div class="permission-approve-actions" role={gate.kind === 'select' ? 'group' : undefined} aria-label={gate.kind === 'select' ? gate.title : undefined}>
                      {#if gate.kind === 'confirm'}
                        <button disabled={answerState?.pending} onclick={() => answerPermission(message.run, { type: 'confirm', value: true })}>Allow</button>
                      {:else if gate.kind === 'select'}
                        {#each choices.options as option}
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
                  <RunMark stage={message.run.stage} settled={settledPhases.has(message.run.phase)} />
                  <div class="response-meta">
                    <ExecutionTime startedAt={message.run.startedAt} />
                    <span class="copy-slot" aria-hidden="true"></span>
                  </div>
                </div>
              {/if}
              {#if message.run.phase === 'complete'}
                {@const summary = receiptSummary(message.run.receipt, providerNames)}
                {@const provider = receiptProvider(message.run.receipt)}
                {@const rows = receiptRows({ ...message.run.receipt, routing: [] }, { tools: !parts.some((part) => part.type === 'actions' && actionGroups(part.activities, false).length) })}
                {@const routing = routedTurns(message.run.receipt)}
                {@const columns = receiptUsageColumns(message.run.receipt, providerNames)}
                {@const hasDetails = rows.length > 0 || columns.length > 0 || routing.length > 0}
                {@const recorded = summary.route !== null || summary.model !== null || summary.time !== null || hasDetails}
                {@const expanded = hasDetails && expandedReceipts.has(message.run.id)}
                {@const failure = copyFailure(copy, message.run.id, modifierLabel)}
                <!-- One line: the receipt, then §3.2's action row at its right, copy only in this slice. -->
                <div class="receipt-line">
                  {#if recorded}
                    {#if hasDetails}
                      <button class="provenance" aria-expanded={expanded} aria-label={`${expanded ? 'Collapse' : 'Expand'} receipt: ${receiptLabel(message.run.receipt, providerNames)}`} onclick={() => toggleReceipt(message.run.id)}><span class:expanded class="receipt-marker" aria-hidden="true"></span>{#if summary.route !== null}<span class="route-segment">{summary.route}</span>{/if}{#if summary.model !== null}{#if summary.route !== null}{' '}<span aria-hidden="true">→</span>{' '}{/if}{#if provider}<span class="receipt-provider"><ProviderLogo color {provider} size={12} /></span>{/if}<span>{summary.model}</span>{/if}</button>
                    {:else}
                      <!-- One row adds nothing beyond the line: a clock stands where the chevron would, and nothing expands. -->
                      <p class="provenance" aria-label={`Receipt: ${receiptLabel(message.run.receipt, providerNames)}`}>{#if summary.route !== null}<span class="route-segment">{summary.route}</span>{/if}{#if summary.model !== null}{#if summary.route !== null}{' '}<span aria-hidden="true">→</span>{' '}{/if}{#if provider}<span class="receipt-provider"><ProviderLogo color {provider} size={12} /></span>{/if}<span>{summary.model}</span>{/if}</p>
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
                      <div><dt>{row.label}</dt><dd class:route-value={row.route}>{row.value}</dd></div>
                    {/each}
                  </dl>
                  {#if routing.length}
                    <RoutingDetails {routing} />
                  {/if}
                {/if}
                {#if failure}<div class="run-error copy-failure">{failure}</div>{/if}
              {/if}

            </div>{/if}{/if}
          {/each}
          <i class="transcript-edge" hidden></i>
          {#if openApprovals.length}<div class="thread-approvals"><ApprovalList {tauri} inline approvals={openApprovals} agents={roster} {swarm} {runtimeList} live={liveActivity}
            onanswered={(approval, result) => { if (result?.status === 'answered') swarm = { ...swarm, approvals: applyApproval(swarm.approvals, { approval: { ...(result?.approval ?? approval), state: 'answered' } }) } }} /></div>{/if}
        </div>
        <!-- Always mounted so the region is live before text lands in it; atomic so each
             run phase is read as one sentence, and never re-read per streamed chunk. -->
        <p class="visually-hidden" aria-live="polite" aria-atomic="true" data-testid="run-announcement">{announcement}</p>
        </div>
        <ChatComposer bind:element={composerBox}>
        {#if rewindNotice}
          <section aria-label="Rewind result" class="rewind-notice" role={rewindNotice.done ? 'status' : 'alert'}>
            <p>{#if !rewindNotice.done}<LucideIcon name="triangle-alert" size={14} />{/if}<strong>{rewindNotice.headline}</strong>{#if rewindNotice.notCovered.length}<span>Not covered: <span class="mono">{rewindNotice.notCovered.join(', ')}</span>.</span>{/if}</p>
            {#if rewindNotice.details.length}<details open={!rewindNotice.done}><summary>{rewindNotice.done ? 'Details' : 'What was not restored'}</summary><ul>{#each rewindNotice.details as line}<li>{line}</li>{/each}</ul></details>{/if}
            <PopupClose label="Dismiss rewind result" onclick={() => { rewindNotice = null }} />
          </section>
        {/if}
        {#if retryReview}
          <section aria-label="Review retry" class="retry-review">
            <p>Retry sends this message again in this conversation.</p>
            {#if retryReview.ranTools}<p>Earlier actions are not undone. Sending may run tools again.</p>{/if}
            {#if retryReview.chooseModel}<button type="button" onclick={() => openComposerPanel('model')}>Choose a different model for this retry</button>{/if}
            <button type="button" onclick={() => { retryReview = null; draft = ''; pickerOpen = false }}>Cancel retry</button>
          </section>
        {/if}
        {#if contextOpen && contextUsage}<ContextPanel usage={contextUsage} active={!!active} {compacting} error={compactError} oncompact={(instructions) => compactThread(instructions)} onsettings={() => openSettings('context')} onclose={() => { contextOpen = false; compactError = '' }} />{/if}
        {#if pickerOpen}
          <ModelPicker {inventory} {contextUsage} current={chipModel} source={modelSource} actions={modelActions} onaction={runModelAction} onchoose={chooseModel} onvisibility={setModelShown} onmanage={openModelSettings} onrouting={() => openSettings('decisions')} onclose={closePicker} />
        {/if}
        {#if thinkingOpen && chipThinking}
          <ThinkingPicker levels={chipThinking.levels} current={chipThinking.level} onchoose={chooseThinkingLevel} onclose={closeThinking} />
        {/if}
        {#if modeOpen && composerModeShown}
          <ModePicker modes={modeListing.listing.modes} current={composerMode.id} source={modeSource} onchoose={chooseMode} onclose={closeModes} />
        {/if}
        {#if dictation.state === 'modelNotInstalled' && !speechInstallDismissed}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <section data-composer-panel data-panel="speech" data-panel-variant="overlay" class="speech-install-popover" aria-labelledby="speech-install-title" onkeydown={speechInstallKeydown}>
            <header class="speech-install-head">
              <strong id="speech-install-title">Voice dictation</strong>
              <PopupClose label="Close voice dictation" onclick={dismissSpeechInstall} />
            </header>
            {#if speechInstallFacts}
              <p class="speech-install-lead">Dictation runs on this device after a one-time <strong class="nowrap">{formatBytes(speechInstallFacts.totalDownloadBytes)}</strong> download.</p>
              <details class="speech-install-details">
                <summary>Details</summary>
                <dl>
                  <div><dt>Free disk required</dt><dd>{formatBytes(speechInstallFacts.requiredFreeBytes)}</dd></div>
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
              <!-- The Install control states a missing model, so only a later state earns a line. -->
              {#if speechInstallStatus && speechInstallStatus.state !== 'notInstalled'}<p role="status">{installStateWords(speechInstallStatus.state)}</p>{/if}
              {#if speechInstallStatus?.state === 'notInstalled' || speechInstallStatus?.state === 'cancelled' || speechInstallStatus?.state === 'failed'}
                <button type="button" disabled={speechInstallPending} onclick={() => startSpeechInstall()}>Install</button>
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
          <!-- The changed-files chip and the jump to the latest message sit side by side, centered above the composer. -->
          {#if changed.length || (!pinned && hasContentBelow)}
            <div class="composer-chips">
              <FileChanges files={changed} onopen={openFile} onreview={!threadGroup && !currentSummary?.replica && reviewTurns.length ? () => { reviewing = { runId: null } } : null} />
              {#if !pinned && hasContentBelow}<button type="button" class="latest" aria-label="Jump to latest" onclick={jumpToLatest}><LucideIcon name="arrow-down" variant="action" size={14} /></button>{/if}
            </div>
          {/if}
          <ComposerNotice notices={composerNotices} />
          {#if pendingCreation}
            <div class="creation-chip" role="group" aria-label="Template" data-tooltip={`Goal: ${pendingCreation.goal}`}>
              <LucideIcon name={pendingCreation.kind === 'agent' ? 'bot' : 'file-code'} size={14} />
              <span class="creation-chip-name">{creationName(pendingCreation.kind, pendingCreation)}</span>
              <span class="visually-hidden">Goal: {pendingCreation.goal}. Output: {pendingCreation.output}.</span>
              <PopupClose label="Cancel creation" onclick={cancelCreation} />
            </div>
          {/if}
          {#if selectedFiles.length}
            <ul class="attachments" aria-label="Selected files">
              {#each selectedFiles as file}
                <li>
                  <LucideIcon name={file.isDirectory ? 'folder' : 'file-text'} size={14} />
                  {#if file.isDirectory}<span class="attachment-name">{file.displayName}</span>
                  {:else}<button type="button" class="attachment-name" onclick={() => openFile({ path: file.path })}>{file.displayName}</button>{/if}
                  <PopupClose label={`Remove ${file.displayName}`} onclick={() => { selectedFiles = selectedFiles.filter(({ path }) => path !== file.path) }} />
                </li>
              {/each}
            </ul>
          {/if}
          <div class="composer-input">
            {#if mention && threadGroup}<AgentMentions agents={mentionAgents} selected={mentionSelected} onchoose={chooseAgentMention} />{:else if mention}<FileMentions rootLabel={workspaceDirectory || (selectedProject ? 'Project folder' : currentThreadId ? 'Thread workspace' : 'Muniment folder')} files={mentionFiles} loading={mentionLoading} error={mentionError} selected={mentionSelected} onchoose={chooseMention} />{/if}
            <ComposerReferences commands={extensionCommandNames} text={draft} references={selectedFiles.map((file) => file.referenceName)} scrollTop={composerScrollTop} scrollLeft={composerScrollLeft} width={composerTextWidth} />
            <label class="visually-hidden" for="composer-message">Message</label>
            <textarea id="composer-message" class="reference-input" data-focus-frame aria-autocomplete="list" aria-controls={mention ? (threadGroup ? 'agent-mentions' : 'file-mentions') : undefined} aria-activedescendant={threadGroup ? (mentionAgents[mentionSelected] ? `agent-mention-${mentionSelected}` : undefined) : mentionFiles[mentionSelected] ? `file-mention-${mentionSelected}` : undefined} onscroll={syncComposerScroll} onclick={updateMention} onkeyup={(event) => { if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) updateMention() }} onblur={() => closeMentions()} aria-describedby={threadSwitching || isDictationActive(dictation) || composerHint ? 'composer-hint' : undefined} bind:this={composer} use:focusComposerOnMount bind:value={draft} oninput={composerInput} onkeydown={keydown} rows="2" placeholder={resumingReply ? 'Resuming interrupted reply…' : agentChat ? (agentChat.record ? 'Ask for a change' : 'Describe your agent, for example: Every weekday at 9am, summarize my inbox') : 'Ask anything'} disabled={composer && (resumingReply || threadSwitching)}></textarea>
          </div>
          {#if draftLinks.length}<div class="composer-links" aria-label="Links in message">{#each draftLinks as url (url)}<button type="button" onclick={() => openUrl(url).catch(() => { submitError = 'This link could not be opened.' })}><LucideIcon name="globe" /><span>{url}</span></button>{/each}</div>{/if}
          {#if submitError}<p class="cancel-error" role="alert">{submitError}</p>{/if}
          {#if cancelError}<p class="cancel-error" role="alert">{cancelError}</p>{/if}
          {#if queueError}<p class="cancel-error" role="alert">{queueError}</p>{/if}
          <QueuedNotice op={queuedOp} history={composerFleetLine} moveSuggested={!!threadAgent?.move_suggested} error={fleet.state.error} oncancel={async (op) => { if (await fleet.cancel(op.op_id) && pendingOffload?.opId === op.op_id) pendingOffload = null }} onmove={moveAgentHere} />
          {#if unreachableMachine}
            <section class="update-notice" aria-live="polite">
              <p class="record error-record">{unreachableMachine.name}: {statusLabel(unreachableMachine)}</p>
              <p class="support">This thread lives on {unreachableMachine.name}. {statusHint(unreachableMachine)}</p>
            </section>
          {/if}
          {#if runtimeUpgradePending()}
            <section class="update-notice" aria-live="polite">
              <p class="record error-record">A Muniment update is finishing.</p>
              <p class="support">Muniment resumes on its own.</p>
            </section>
          {/if}
          {#if composerWarning}<LimitBanner limit={composerWarning} onview={() => toggleFullPage('usage')} />{/if}
          <div class="composer-row" bind:this={composerRow}>
            <div class="composer-meta">
            <ComposerExtensions bind:commandNames={extensionCommandNames} bind:this={composerExtensions} bind:selection={turnExtensions} {tauri} threadId={currentThreadId} active={!!active} bind:draft onmanage={() => openSettings('extend')} onfiles={chooseFiles} filesNote={remoteRuntime ? REMOTE_FILES_NOTE : ''} />
            <MachinePicker list={runtimeList} locked={threadMachine} source={machineSource} owner={currentOwner} fleet={fleetTargets} offload={offloadTarget} disabled={switchLocked || !!active || !!pendingOffload} onselect={(runtime) => { offloadTarget = null; machineLayer = 'thread'; void selectRuntime(runtime) }} onoffload={(target) => { offloadTarget = target }} onmanage={() => openSettings('runtimes')} />
            {#if threadGroup}
              <GroupMembers members={threadGroupMembers} live={(agent) => roster.find(entry => entry.id === agent.id)?.activity} />
            {:else if auth.name === 'local'}
              <ComposerMenuButton menu="model" open={pickerOpen} chevrons haspopup="dialog" label={`Model: ${modelSourceLabel}`} text={modelSourceLabel} compact={chipModel ? null : unconnectedModelIcon} bind:element={modelChip} onclick={togglePicker}>
                {#snippet icon()}{#if chipModel}<ProviderLogo provider={inventory?.router_models?.find(entry => entry.id === chipModel.model)?.family || (chipModel.model === 'auto' ? classifierProvider(inventory?.router_classifier) : null) || chipModel.provider} size={14} />{/if}{/snippet}
              </ComposerMenuButton>
              {#if chipThinking}
                <ComposerMenuButton menu="thinking" open={thinkingOpen} label={`Thinking level: ${thinkingLabel(chipThinking.level)}`} text={thinkingLabel(chipThinking.level)} bind:element={thinkingChip} onclick={() => openComposerPanel(thinkingOpen ? null : 'thinking')}>
                  {#snippet icon()}<LucideIcon name="brain" size={14} />{/snippet}
                </ComposerMenuButton>
              {/if}
            {/if}
            {#if composerModeShown}
              <ComposerMenuButton menu="mode" open={modeOpen} chevrons label={`Permission mode: ${composerMode.title}`} text={composerMode.title} bind:element={modeChip} onclick={() => openComposerPanel(modeOpen ? null : 'mode')}>
                {#snippet icon()}<LucideIcon name={modeIcon(composerMode)} size={14} />{/snippet}
              </ComposerMenuButton>
            {/if}
            {#if compacting || contextOpen || contextMeterShown(contextUsage)}<ContextMeter usage={contextUsage} open={contextOpen} {compacting} onclick={() => openComposerPanel(contextOpen ? null : 'context')} />{/if}
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
              <button type="button" class="quiet composer-icon" aria-pressed={isDictationActive(dictation)} aria-keyshortcuts={ariaKeyShortcut(globalVoiceShortcutValue)} disabled={voiceDisabled} onpointerdown={voicePointerDown} onpointerup={voicePointerEnd} onpointercancel={voicePointerEnd} onkeydown={voiceKeyDown} onkeyup={voiceKeyUp} onclick={voiceClick} aria-label="Voice" aria-haspopup={dictation.state === 'modelNotInstalled' ? 'dialog' : undefined} aria-expanded={dictation.state === 'modelNotInstalled' ? !speechInstallDismissed : undefined}><LucideIcon name="mic" variant="action" size={16} /></button>
              {#if active || outgoingDraft().trim()}
                <button type="button" class="composer-action primary" class:stop={!!active} aria-label={active ? 'Stop' : 'Send'} disabled={threadSwitching} aria-disabled={composerActionInactive() ? 'true' : undefined} onclick={composerActionClick}>
                  <LucideIcon name={active ? 'square' : 'arrow-up'} variant="action" size={active ? 12 : 16} />
                </button>
              {:else}<span class="composer-action-slot" aria-hidden="true"></span>{/if}
            </div>
          </div>
          {#if dictationError && dictation.state !== 'modelNotInstalled' && !speechInstallNotice}<div class="dictation-error" role="alert">{dictationError}</div>{/if}
        </ChatComposer>
        </div>
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
          ><span class="divider-grip" aria-hidden="true"><LucideIcon name="ellipsis-vertical" size={14} variant="action" /></span></div>
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
            ><span class="divider-grip" aria-hidden="true"><LucideIcon name="ellipsis-vertical" size={14} variant="action" /></span></div>
          {/if}
          <RecordPanel {tauri} maximized={recordMaximized} refresh={recordRefresh} ontogglemaximized={toggleRecordMaximized} onask={askAboutView} request={recordRequest} />
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
            ><span class="divider-grip" aria-hidden="true"><LucideIcon name="ellipsis-vertical" size={14} variant="action" /></span></div>
          {/if}
          {#await loadFilePanel() then module}{@const FilePanel = module.default}<FilePanel file={viewedFile} {tauri} threadId={currentThreadId} maximized={recordMaximized} ontogglemaximized={toggleRecordMaximized} onclose={closeRail} />{/await}
        {/if}
    {#if agentProfileOpen && profileAgent && !agentsOpen}
      <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
      <div class="artifact-divider agent-divider" role="separator" aria-label="Agent profile width" aria-orientation="vertical" aria-valuemin="240" aria-valuemax={agentPanelMaximum} aria-valuenow={agentPanelWidth} tabindex="0" onpointerdown={agentResize.pointerDown} onpointermove={agentResize.pointerMove} onpointerup={agentResize.pointerEnd} onpointercancel={agentResize.pointerEnd} onkeydown={agentResize.keydown}><span class="divider-grip" aria-hidden="true"><LucideIcon name="ellipsis-vertical" size={14} variant="action" /></span></div>
      {#key `${profileAgent.runtimeId}:${profileAgent.id}`}<AgentProfile agent={profileAgent} {tauri} projects={projectRows} run={agentListing.latestRuns[profileAgent.id]}
        runs={activityRuns.filter(entry => entry.agent_id === profileAgent.id && entry.runtimeId === profileAgent.runtimeId)} agents={roster} {runtimeList} {swarm} {inventory} catalog={agentCatalog} activity={profileAgent.activity} threads={agentListing.threads?.[profileAgent.id] ?? []}
        onclose={() => { agentProfileOpen = false; profileOnlyAgent = null }} onchange={refreshAgentsAndRuns} onrun={(run) => { startedRuns.add(run.id); activityRuns = applyRunUpdate(activityRuns, { runtimeId: run.runtimeId, run }) }}
        onmanagemodels={openModelSettings}
        ondelete={() => { agentProfileOpen = false; selectedAgent = null; profileOnlyAgent = null; void refreshAgents() }}
        onopen={(id) => showThread(id)} onopenrun={(run) => openRunThread(run).catch(error => { submitError = String(error) })} onactivity={() => showNotifications()} />{/key}
    {/if}
  {#if browserPanel}<div class="artifact-divider workspace-divider" role="separator" aria-label="Workspace panel width" aria-orientation="vertical" aria-valuemin="340" aria-valuemax={workspacePanelMaximum} aria-valuenow={workspacePanelWidth} tabindex="0" onpointerdown={workspaceResize.pointerDown} onpointermove={workspaceResize.pointerMove} onpointerup={workspaceResize.pointerEnd} onpointercancel={workspaceResize.pointerEnd} onkeydown={workspaceResize.keydown}><span class="divider-grip" aria-hidden="true"><LucideIcon name="ellipsis-vertical" size={14} variant="action" /></span></div>{/if}
  <WorkspacePanel ondirty={value => { workspaceDirty = value }} context={fileContext} {requestedArtifact} {tauri} onfolder={path => { workspaceDirectory = path; if (mention) updateMention() }} navigation={requestedNavigation} onnavigationhandled={request => { if (requestedNavigation === request) requestedNavigation = null }} selected={browserPanel} onselect={showBrowser} threadId={currentThreadId} projectId={selectedProject} requestedFile={requestedWorkspaceFile} suspended={!!deletingThreadId} />
  {#if fullPage === 'usage'}<UsagePage {tauri} {runtimeList} {folderAccess} onconnect={() => openSettings('providers')} />{/if}
  {#if fullPage === 'notifications'}<NotificationsPage {tauri} {runtimeList} agents={roster} {swarm} refreshKey={notificationsKey} onread={() => { void refreshNeedsYou() }} onopen={(run) => openRunThread(run).catch(error => { submitError = String(error) })}
    onapprovalopen={(approval) => openRunThread(approval).catch(error => { submitError = String(error) })} />{/if}
  {#if paletteOpen && tauri}<CommandPalette {tauri} input={paletteFor} onrun={runPaletteItem} onclose={() => { paletteOpen = false }} />{/if}
  {#if moveTarget}<ProjectPicker title={moveTarget.title} projects={moveProjects} current={projectCatalog.threads[moveTarget.threadId] ?? null} busy={moveBusy} error={moveError} onmove={(projectId) => moveThread(moveTarget.threadId, projectId)} onclose={() => { moveTarget = null }} />{/if}
  {#if groupCreateOpen}<GroupCreate {tauri} agents={roster} {swarm} onclose={() => { groupCreateOpen = false }} oncreated={async (group) => { await refreshSwarm(); await chatController.refreshThreads(); await openGroup(group).catch(error => { submitError = String(error) }) }} />{/if}
  {#if agentsOpen}{#key agentsRequest}
    <AgentManager {tauri} {runtimeList} agents={roster} onimported={async (agent) => { await refreshAgents(); const imported = roster.find(entry => entry.id === agent.id); if (imported) await openAgent(imported).catch(error => { submitError = String(error) }) }} isArchived={catalogArchived} onaction={catalogAction} {threadSummaries} pending={creations.filter(item=>item.kind==='agent'&&!agentListing.agents.some(agent=>agent.id===item.resultId))} oncreate={(chosen)=>openAgentChat(chosen?.description ? { description: chosen.description } : {})} onselect={(agent) => openAgent(agent).catch(error => { submitError = String(error) })} onopen={openCreation} onchange={(next) => { agentListing = next }}
      {inventory} latestRuns={agentListing.latestRuns} onprofile={openAgentProfile} onmanagemodels={openModelSettings} onrun={(run) => { startedRuns.add(run.id); activityRuns = applyRunUpdate(activityRuns, { runtimeId: run.runtimeId, run }) }} />
  {/key}{/if}
  {#if projectsOpen && settingsProject && sidebarProjectById[settingsProject]}
    <ProjectSettings {tauri} project={sidebarProjectById[settingsProject]} agents={roster.filter(agent => sidebarProjectById[settingsProject].agent_ids.includes(agent.id))} {inventory} modes={modeListing?.listing ?? null} {runtimeList} latestRuns={agentListing.latestRuns} busy={switchLocked || projectBusy}
      onback={() => { projectsOpen = false; settingsProject = null }} onnewthread={newProjectThread} onaddtrigger={addProjectTrigger}
      onchat={(agent) => openAgent(agent).catch(error => { submitError = String(error) })} onprofile={openAgentProfile} onchange={refreshAgentsAndRuns} onmanagemodels={openModelSettings}
      onaction={(agent, action, name) => action === 'profile' ? openAgentProfile(agent) : action === 'export' ? exportAgent(tauri, agent, 'muniment') : catalogAction('agent', agent, action, name)}
      onrun={(run) => { startedRuns.add(run.id); activityRuns = applyRunUpdate(activityRuns, { runtimeId: run.runtimeId, run }) }} />
  {:else if projectsOpen}<ProjectCatalog bind:selected={catalogProject} projects={projectRows} remote={remoteProjectRows} threads={regularThreads.filter(thread => !threadOrganization[thread.threadId]?.archived)} assignments={projectCatalog.threads} busy={switchLocked || projectBusy} error={projectError} loading={moreThreads || loadingOlderThreads} oncreate={createCatalogProject} onrename={async (id, name) => { await tauri.invoke('project_rename', {projectId:id,name}); await refreshProjects() }} onopen={openProjectFolder} onthread={id => threadRowClick({}, id)} onnewthread={newProjectThread} />{/if}
  {#if artifactsOpen}<ArtifactCatalog items={artifactChats} isArchived={catalogArchived} onaction={catalogAction} onopen={openCreation} oncreate={(chosen)=>startCreation('artifact', chosen)}/>{/if}
        <!-- Settings sits in the workspace grid: its section list takes the sidebar column. -->
        {#if settingsOpen}
          {#await loadSettings() then module}
          {@const Settings = module.default}
          <Settings {tauri} bind:section={settingsSection} connectProvider={settingsConnectProvider} onclose={closeSettings} homePath={onboarding.homePath} onchangehome={openHomeSettings} local={auth.name === 'local'} signInDisabled={!!active || !!backgroundRun || localEntryPending} onsignin={signIn} {accountStatus} inventory={inventoryMachine === (remoteRuntime?.id ?? LOCAL_RUNTIME_ID) ? inventory : null} oninventory={(next) => { inventory = next; inventoryMachine = remoteRuntime?.id ?? LOCAL_RUNTIME_ID; if (accountStatus === inventoryError) accountStatus = ''; inventoryError = '' }} oncompanieschange={() => { recordRefresh += 1 }} voiceShortcut={globalVoiceShortcutValue} voiceShortcutChanging={globalVoiceChanging} onVoiceShortcutChange={changeVoiceShortcut} defaultVoiceShortcut={holdToTalkShortcut()} oncreateextension={createExtensionDraft} subject={auth.subject ?? ''} onsignout={() => run('sign-out')} onrecord={openRecordFromSettings} {remoteRuntime} {runtimeList} onselectruntime={(runtime) => { offloadTarget = null; void selectRuntime(runtime) }} runtimeSwitchDisabled={switchLocked || !!active || !!backgroundRun} onpermissions={() => { modesRevision += 1 }} />
          {:catch}<p role="alert">Settings could not load. Close and reopen settings.</p>{/await}
        {/if}
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

  {#if reviewing && currentThreadId}{#await loadReviewChanges() then module}{@const Review = module.default}<Review {tauri} runtime={composerRuntime} threadId={currentThreadId} turns={reviewTurns} initialRunId={reviewing.runId} onclose={() => { reviewing = null }} />{/await}{/if}
  {#if rewinding && currentThreadId}{#await loadRewindDialog() then module}{@const Rewind = module.default}<Rewind {tauri} runtime={composerRuntime} threadId={currentThreadId} runId={rewinding.runId} prompt={rewinding.prompt} ondone={finishRewind} onclose={() => { rewinding = null }} />{/await}{/if}
  {#if deletingThreadId}
    <ConfirmDialog title={deletingThreadIds.length > 1 ? `Delete ${deletingThreadIds.length} threads?` : `Delete ${threadSummaries.find(t => t.threadId === deletingThreadId)?.title || 'this thread'}?`} cancelLabel="Cancel" confirmLabel={deletingThreadIds.length > 1 ? `Delete ${deletingThreadIds.length} threads` : 'Delete'} onDecision={approve => approve ? confirmDeleteThread(deletingThreadId) : cancelDeleteThread()}>
      {#if deletingThreadIds.length > 1}
        {@const titles = deletingThreadIds.map(id => threadSummaries.find(t => t.threadId === id)?.title || 'New thread')}
        <p>These threads and their messages will be deleted:</p>
        <ul class="delete-titles">{#each titles.slice(0, 5) as title, index (index)}<li>{title}</li>{/each}{#if titles.length > 5}<li>{titles.length - 5} more</li>{/if}</ul>
      {:else}<p>This thread and its messages will be deleted.</p>{/if}
    </ConfirmDialog>
  {/if}
{#if pairingRequests[0]}
  {#key pairingRequests[0]}
    <ConfirmDialog title="Approve Muniment connection" onDecision={decidePairing}>
      <p>Allow this program to access workspace {pairingRequests[0].workspace}?</p>
      <p>Permissions: {pairingRequests[0].scopes.map(scope => ({ 'run.write': 'run tasks', 'thread.read': 'read workspace data' })[scope] ?? scope).join(' and ') || 'none'}.</p>
      <p>Identifies as {pairingRequests[0].claimedKind} {pairingRequests[0].claimedVersion}.</p>
    </ConfirmDialog>
  {/key}
{/if}

<style>

  .sign-in-overlay { position: fixed; z-index: 100; top: 72px; left: 50%; transform: translateX(-50%); width: min(480px, calc(100% - 48px)); padding: 20px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--paper); box-shadow: var(--shadow-overlay); }
  main {
    min-height: 100vh;
    display: grid;
    place-content: center;
    justify-items: center;
  }

  /* The block from the mark to the Home line is centered, and the connect list
     scrolls when the window is shorter than the block. */
  main.onboarding-active {
    height: 100vh;
    min-height: 0;
    grid-template-rows: auto minmax(0, auto);
    grid-template-columns: minmax(0, 1fr);
    align-content: center;
    box-sizing: border-box;
    padding: 24px;
  }

  /* The pocket-fold symbol stands over the wordmark, as in the stacked brand lockup. */
  .lockup {
    display: grid;
    justify-items: center;
    gap: 14px;
  }

  /* On the first run the symbol's art is as wide as the wordmark. */
  main.onboarding-active .lockup {
    gap: 6px;
  }

  .name {
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
  .primary { background: var(--accent); border-color: var(--accent); color: var(--on-accent); }
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
    background: color-mix(in srgb, var(--accent) 86%, var(--ink));
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
  .workspace { --frame-width: 4px; --titlebar-band: 36px; --titlebar-height: 30px; position: fixed; inset: 0; display: grid; grid-template-rows: var(--titlebar-height) minmax(0, 1fr); padding: 0 var(--frame-width) var(--frame-width); row-gap: calc(var(--titlebar-band) - var(--titlebar-height)); column-gap: var(--frame-width); background: var(--paper); }
  /* The sidebar column is the element's --sidebar-column: the kept width, or zero collapsed, and the 180ms slide carries both. */
  .workspace { grid-template-columns: minmax(0, var(--sidebar-column)) minmax(0, 1fr); grid-template-areas: "title title" "side thread"; transition: grid-template-columns 180ms ease; }
  .creation-goal { padding:12px 0; font-size:var(--text-13); color:var(--muted); } .creation-goal p { margin:4px 0 0; } .creation-goal summary, .creation-request summary { cursor:pointer; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; } .creation-goal strong, .creation-request strong { color:var(--ink); font-weight:600; } .creation-request p { margin:6px 0 0; color:var(--muted); font-size:var(--text-13); } .creation-chip { display:inline-flex; align-items:center; gap:6px; max-width:100%; min-height:28px; margin:0 0 6px; padding:0 4px 0 8px; border:1px solid var(--border); border-radius:var(--radius-control); background:var(--faint); color:var(--ink); font-size:var(--text-13); } .creation-chip-name { min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; } .creation-chip { position:relative; } .creation-chip::after { content:attr(data-tooltip); position:absolute; left:0; bottom:calc(100% + 6px); z-index:6; width:max-content; max-width:min(420px, 80vw); padding:6px 9px; border:1px solid var(--border); border-radius:var(--radius-control); background:var(--surface); color:var(--ink); font-size:var(--text-12); white-space:normal; pointer-events:none; opacity:0; transition:opacity 80ms ease; } .creation-chip:hover::after { opacity:1; transition-delay:200ms; } .creation-suggestions { display:flex; flex-wrap:wrap; gap:8px; } .creation-suggestions button[aria-pressed="true"] { border-color:var(--ink); }
  .workspace.artifact-resizing, .workspace.sidebar-resizing { transition: none; }
  /* The sidebar yields frame space at the window minimum while the thread keeps 320px. */
  /* The agent's title pill: a wash of the agent's color with the accent edge in it. */
  .titlebar .quiet.agent-chat-title { flex: 0 1 auto; display: inline-flex; align-items: center; gap: 6px; min-width: 0; max-width: 100%; padding-left: 6px; background: color-mix(in srgb, var(--agent-color) 20%, transparent); }
  .titlebar .quiet.agent-chat-title:hover:not(:disabled) { background: color-mix(in srgb, var(--agent-color) 32%, transparent); }
  .agent-chat-title span { overflow: hidden; white-space: nowrap; }
  .agent-chat-title span[data-overflow="true"] { mask-image: linear-gradient(to right, currentColor calc(100% - 28px), transparent); }
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
  .workspace.macos .titlebar { display: grid; grid-template-columns: subgrid; column-gap: var(--frame-width); margin: 0; padding: 0; }
  /* The sidebar part is at least as wide as its controls, so a narrow sidebar column never hides the toggle. */
  .workspace.macos .titlebar-sidebar { grid-column: 1; position: relative; z-index: 1; box-sizing: content-box; display: flex; align-items: end; gap: 8px; min-width: calc(var(--titlebar-controls-end) - var(--titlebar-inset)); padding-left: calc(var(--titlebar-inset) - var(--frame-width)); }
  /* Artifacts sits flush right: 4px inside the 8px frame matches the 12px row padding elsewhere. */
  .workspace.macos .titlebar-thread { grid-column: 2 / -1; display: flex; align-items: end; gap: 8px; min-width: 0; padding-left: max(0px, calc(var(--titlebar-controls-end) - var(--sidebar-column) - 2 * var(--frame-width))); padding-right: calc(12px - var(--frame-width)); }
  /* Artifacts then Record sit flush right, the update control beside them. */
  .workspace.macos .update-slot { order: 1; }
  .workspace.macos .titlebar-thread :global(.record-toggle) { order: 3; }
  .titlebar button, .titlebar input { min-width: 24px; min-height: 24px; height: 24px; padding: 0 6px; }
  .titlebar .quiet { flex: none; display: inline-flex; align-items: center; justify-content: center; gap: 6px; }
  .titlebar button:hover:not(:disabled) { background: var(--faint); border-color: transparent; }
  /* A hairline edge keeps the chip legible over the control's --faint hover. */
  .titlebar kbd { margin-left: 2px; padding: 0 4px; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--faint); }
  .update-slot { flex: 0 0 auto; }
  .catalog-title { padding: 0 7px; font-family: var(--font-heading); font-weight: 600; line-height: 24px; white-space: nowrap; }
  .thread-title-heading { display: flex; min-width: 24px; max-width: 100%; margin: 0; font: inherit; font-family: var(--font-heading); }
  /* The rename field keeps the title control's register while it shows. */
  /* Editing is the rename control's active state: the composer's muted hairline, no ring. */
  input.thread-title { flex: 0 1 320px; max-width: 100%; overflow: hidden; border: 1px solid var(--muted); outline: 0; background: transparent; color: var(--ink); font: inherit; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; user-select: text; }
  kbd { margin-left: 10px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .title-spacer { flex: 1; align-self: stretch; min-width: 24px; }
  .sidebar { grid-area: side; min-width: 0; display: flex; flex-direction: column; padding: 0; }
  .side-scroll { flex: 1; min-height: 0; padding: 4px 6px 8px; overflow-y: auto; }
  .project-heading { display: flex; align-items: center; justify-content: space-between; margin: 8px 8px 2px; }
  .project-heading h3 { margin: 0; color: var(--muted); font: var(--weight-semibold) var(--text-12) var(--font-human); }
  .project-heading button { padding: 2px; }
  .side-top { padding: 8px 6px 0; }
  .side-top kbd { flex: none; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .sidebar-head { display: flex; align-items: center; gap: 4px; container-type: inline-size; }
  /* A narrow sidebar keeps the search words and drops the shortcut chip, which aria-keyshortcuts still names. */
  @container (max-width: 250px) { .sidebar-head .thread-search-field .palette-open { display: none; } .sidebar-head .thread-search-field:has(kbd) .thread-search { padding-right: 8px; } }
  .sidebar-head .new-thread { flex: none; width: auto; padding-right: 10px; }
  .library-entry kbd { flex: none; margin-left: auto; }
  .thread-search-field { position: relative; flex: 1; min-width: 0; }
  .thread-search-field:has(kbd) .thread-search { padding-right: 40px; }
  .thread-search-field .palette-open { position: absolute; right: 4px; top: 50%; display: inline-flex; align-items: center; min-width: 24px; min-height: 24px; margin: 0; padding: 0 4px; transform: translateY(-50%); border: 0; border-radius: var(--radius-control); background: transparent; cursor: pointer; }
  .thread-search-field .palette-open:hover { background: var(--faint); }
  .thread-search-field kbd { margin: 0; pointer-events: none; }
  .thread-search-field :global(.lucide) { position: absolute; left: 8px; top: 50%; transform: translateY(-50%); pointer-events: none; color: var(--muted); }
  .thread-search { width: 100%; min-width: 0; box-sizing: border-box; margin: 0; padding: 6px 8px 6px 30px; border: 0; border-radius: var(--radius-control); background: var(--faint); color: var(--ink); font: var(--text-13) var(--font-human); }
  .retry-review { font-size: var(--text-12); color: var(--muted); padding-bottom: 8px; border-bottom: 1px solid var(--border); }
  .rewind-notice { position: relative; padding: 0 36px 8px 0; border-bottom: 1px solid var(--border); color: var(--muted); font-size: var(--text-12); }
  .rewind-notice p { display: flex; flex-wrap: wrap; align-items: center; gap: 0 6px; margin: 0; }
  .rewind-notice p > :global(.lucide) { color: var(--warning); }
  .rewind-notice strong { color: var(--ink); font-weight: var(--weight-semibold); }
  .rewind-notice .mono { font-family: var(--font-mono); }
  .rewind-notice ul { margin: 4px 0 0; padding-left: 18px; font-family: var(--font-mono); overflow-wrap: anywhere; }
  .rewind-notice > :global(.popup-close) { position: absolute; top: -4px; right: 0; }
  .message-actions button[aria-disabled="true"] { opacity: .45; cursor: default; }
  /* Lucide's undo-2, drawn as a mask: a long transcript carries one per message. */
  .message-actions .rewind-turn::before { content: ''; width: 14px; height: 14px; background: currentColor; -webkit-mask: var(--rewind-icon) center / 14px no-repeat; mask: var(--rewind-icon) center / 14px no-repeat; }
  .message-actions .rewind-turn { --rewind-icon: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24' fill='none' stroke='black' stroke-width='1.6' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M9 14 4 9l5-5'/%3E%3Cpath d='M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11'/%3E%3C/svg%3E"); min-width: 24px; min-height: 24px; }
  .side-empty { padding: 4px 8px; color: var(--muted); font-size: var(--text-12); }
  .side-bottom { flex: none; padding: 0 6px; }
  .side-action :global(.lucide) { flex: 0 0 18px; width: 18px; height: 18px; }
  .side-action:hover { background: var(--faint); }
  .side-action[aria-pressed="true"], .side-action[aria-current="true"] { background: var(--selected); color: var(--on-selected); }
  /* An agent's row carries its color: a wash on hover and while open, with an edge in that color. */
  .row-rename { display: flex; flex-wrap: wrap; gap: 4px; padding: 4px; }
  .row-rename input { width: 100%; min-width: 0; font: inherit; }
  .row-rename button { padding: 2px 6px; font-size: var(--text-12); }
  /* Clip labels during the panel slide without clipping the profile popover. */
  .older-threads, .side-action span { overflow: hidden; }
  .side-toggle { line-height: 0; }
  .side-toggle:hover:not(:disabled) { border-color: transparent; background: var(--faint); }
  .side-toggle:hover:not(:disabled) :global(.side-icon) { color: var(--ink); }
  .side-action, .thread-row { width: 100%; display: flex; align-items: center; gap: 8px; min-height: var(--row-height); padding: var(--row-pad) 8px; border-color: transparent; background: transparent; text-align: left; }
  /* The bar holds the Threads heading's margins and 24px height. */
  .selection-bar { display:flex; flex:1; min-width:0; align-items:center; gap:5px; height:28px; margin:0; font-size:var(--text-12); }
  .selection-bar span { flex:1; color:var(--muted); font-family:var(--font-human); }
  .selection-bar button { min-width:24px; min-height:24px; padding:0 6px; font-size:var(--text-12); }
  .thread-row > .select-mark { flex:0 0 14px; display:inline-flex; color:var(--muted); }
  .thread-row.selected > .select-mark { color:var(--ink); }
  .delete-titles { margin:4px 0 0; padding-left:18px; }
  .older-threads { width: 100%; margin-top: 4px; border-color: transparent; background: transparent; color: var(--muted); }
  .thread-record { position: relative; }
  .thread-row { font: inherit; font-size: var(--text-13); color: var(--ink); border: 1px solid transparent; border-radius: var(--radius-control); user-select: none; -webkit-user-select: none; }
  /* Hover is a lighter wash than the open thread's faint row, as a selection is darker, so the three never look alike. */
  button.thread-row:hover:not([aria-disabled="true"]) { background: color-mix(in srgb, var(--faint) 55%, transparent); }
  /* A selected row reads darker than the open thread's faint row, so a selection and the open thread never look alike. */
  .thread-row.selected { background: color-mix(in srgb, var(--selected-edge) 18%, var(--surface)); }
  /* The row's actions control shows at its right end while the pointer or focus rests on the row. */
  .thread-actions { position: absolute; top: 50%; right: 4px; min-width: 22px; min-height: 22px; padding: 0 4px; transform: translateY(-50%); color: var(--muted); opacity: 0; pointer-events: none; }
  .thread-record:hover .thread-actions, .thread-record:focus-within .thread-actions { opacity: 1; pointer-events: auto; }
  .thread-record:hover .thread-row-body, .thread-record:focus-within .thread-row-body { padding-right: 22px; }
  .thread-actions:hover:not(:disabled) { color: var(--ink); background: var(--faint); }
  button.thread-row[aria-disabled="true"] { opacity: .55; }
  /* Inbox rows: a small muted meta line over the title, which fades at its end. The title leads in reading order, so a row's name starts with it. */
  .thread-row-body { flex: 1 1 auto; display: flex; flex-direction: column-reverse; gap: 1px; min-width: 0; }
  .thread-row-meta { display: flex; align-items: center; gap: 4px; min-width: 0; color: var(--muted); font: var(--text-12) var(--font-human); line-height: 16px; white-space: nowrap; }
  .thread-row-meta :global(.lucide) { flex: none; color: var(--muted); }
  .thread-row-meta .row-project, .thread-row-meta .row-agent { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  /* The owner chip keeps its machine name whole while the project name clips first. */
  .thread-row-meta :global(.owner-chip) { flex: none; max-width: 60%; }
  .thread-row time { flex: none; color: var(--muted); font: var(--text-12) var(--font-human); white-space: nowrap; }
  /* The time rests at the right end of the meta line, clear of the actions control. */
  .thread-row-meta time { margin-left: auto; padding-left: 8px; }
  /* The badge keeps its machine name whole while the thread title beside it clips first. */
  .machine-badge { flex: none; display: inline-flex; align-items: center; gap: 4px; min-width: 18px; max-width: 45%; margin-left: auto; padding: 0 5px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font: var(--text-12) var(--font-human); line-height: 16px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .machine-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .machine-dot { flex: none; width: 6px; height: 6px; border-radius: var(--radius-chip); background: var(--border); }
  .machine-dot[data-tone="success"] { background: var(--success); }
  .machine-dot[data-tone="active"] { background: var(--active); }
  .machine-dot[data-tone="warning"] { background: var(--warning); }
  .machine-dot[data-tone="danger"] { background: var(--danger); }
  .thread-row-title { flex: 1 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; }
  .thread-row-title[data-overflow="true"] { mask-image: linear-gradient(to right, currentColor calc(100% - 28px), transparent); }
  .thread-menu { position: fixed; z-index: 4; min-width: 88px; width: max-content; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); }
  .title-thread-actions { flex: none; }
  .thread-menu button { display: block; width: 100%; min-width: 24px; min-height: 24px; padding: 3px 8px; border-color: transparent; background: transparent; color: var(--ink); font-size: var(--text-13); text-align: left; }
  .thread-menu button:hover:not(:disabled) { border-color: transparent; background: var(--faint); }
  /* A narrow sidebar shortens the first control, never its start. */
  /* Sidebar labels overrun the way thread titles do: one line, faded over the last 28px. */
  .side-action span { flex: 1; min-width: 0; white-space: nowrap; }
  .side-action span[data-overflow="true"] { mask-image: linear-gradient(to right, currentColor calc(100% - 28px), transparent); }
  .side-action .side-count { flex: none; min-width: 16px; padding: 0 4px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--ink); font: var(--text-12) var(--font-mono); line-height: 16px; text-align: center; }
  .settings-search { display: flex; align-items: center; gap: 6px; margin: 2px 6px 4px; padding: 0 8px; min-height: 28px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); }
  .settings-search:focus-within { border-color: var(--muted); }
  .settings-search input { flex: 1; min-width: 0; padding: 4px 0; border: 0; outline: none; background: transparent; color: var(--ink); font: var(--text-13) var(--font-human); }
  .side-action.settings-result { padding-left: 34px; color: var(--muted); }
  .settings-empty { margin: 0; padding: 6px 8px; color: var(--muted); font-size: var(--text-13); }
  .settings-back { color: var(--muted); }
  /* The MCP mark takes a Lucide icon's 18px slot and its muted color. */
  /* The foot of the sidebar: Settings above the account row, under one edge-to-edge hairline. */
  .side-foot { position: relative; margin-top: auto; padding: 4px 6px 6px; }
  /* The menu separator: a straight line inset to the foot's 6px padding at three-quarter strength. */
  .side-foot::before { content: ''; position: absolute; top: 0; left: 6px; right: 6px; height: 1px; background: var(--border); opacity: .75; }
  /* Settings and the account share one footer without an internal divider. */
  .settings-block { padding: 0; }
  .side-foot :global(.profile-block) { margin-top: 0; padding-top: 0; border-top: 0; }
  .side-foot :global(.profile-button) { min-height: 28px; padding: 4px 8px; }
  /* The panel is one popup over the thread, never a second settings surface. */
  /* Collapsed means gone: the column is zero wide, the empty panel drops its hairline and padding for the slide, and the thread panel takes the gap. */
  .workspace.sidebar-collapsed .sidebar { padding: 0; border-width: 0; overflow: hidden; }
  .workspace.sidebar-collapsed .thread-panel { margin-left: calc(-1 * var(--frame-width)); }
  /* The open thread's row carries the accent edge: its agent's color, or the theme accent. */
  .active-thread { background: var(--selected); }
  .quiet { background: transparent; border-color: transparent; }
  /* Each divider covers its panel gap with a few pixels of grab on either side. A three-dot grip marks the gap. On hover, focus or drag, the gap fills with the hairline border color, which fades out at both ends. */
  .agent-divider { grid-area: rail; }
  .artifact-divider, .sidebar-divider { --divider-grab: 3px; --divider-glow: 100%; position: relative; z-index: 2; align-self: stretch; justify-self: start; width: calc(var(--frame-width) + 2 * var(--divider-grab)); margin-left: calc(-1 * var(--frame-width) - var(--divider-grab)); padding: 0; border: 0; border-radius: 0; background: linear-gradient(transparent, var(--border) 20%, var(--border) 80%, transparent) center / var(--frame-width) var(--divider-glow) no-repeat; background-color: transparent; background-size: 0 var(--divider-glow); cursor: col-resize; touch-action: none; }
  /* The grip is Lucide's vertical ellipsis, centered on the gap. */
  .divider-grip { position: absolute; top: 50%; left: 50%; display: grid; color: var(--muted); transform: translate(-50%, -50%); pointer-events: none; }
  .artifact-divider:hover, .sidebar-divider:hover, .artifact-divider:focus-visible, .sidebar-divider:focus-visible, .artifact-divider:active, .sidebar-divider:active { background-size: var(--frame-width) var(--divider-glow); }
  .artifact-divider:hover .divider-grip, .sidebar-divider:hover .divider-grip, .artifact-divider:focus-visible .divider-grip, .sidebar-divider:focus-visible .divider-grip, .artifact-divider:active .divider-grip, .sidebar-divider:active .divider-grip { opacity: 0; }
  .artifact-divider { grid-area: rail; }
  .sidebar-divider { grid-area: thread; }
  /* Settings covers the thread and rail. The title band, the sidebar, its divider and the announcement region stay. */
  .workspace.settings-open > :global(:not(.titlebar, .sidebar, .sidebar-divider, .visually-hidden, .settings-layout, .workspace-panel, .workspace-divider, [role="alert"])) { visibility: hidden; }
  /* Settings gives the rail column to an open workspace panel. */
  .workspace.settings-open.tools-open :global(.settings-layout) { grid-column: 2; }
  .workspace.settings-open .titlebar-thread { visibility: hidden; }
  .workspace.settings-open .update-slot, .workspace.settings-open .titlebar-thread :global(.workspace-menu) { visibility: visible; }
  .artifact-rail { grid-area: rail; min-width: 0; padding: 22px 24px; overflow-y: auto; }
  .artifact-rail header { padding-bottom: 15px; border-bottom: 1px solid var(--border); }
  .artifact-rail h2 { margin: 3px 0 0; font-size: var(--text-17); }
  .artifact-empty { display: grid; place-items: center; align-content: center; min-height: 45%; text-align: center; }
  .artifact-empty p { margin: 0; color: var(--muted); }
  /* One grid cell: the thread fills it and the composer sits at its end, so the transcript scrolls on under the composer. */
  .thread-panel { grid-area: thread; position: relative; min-width: 0; display: grid; grid-template-rows: minmax(0, 1fr); grid-template-columns: minmax(0, 1fr); transition: margin-left 180ms ease; }

  .workspace.sidebar-resizing .thread-panel { transition: none; }
  /* The shell clips the transcript and its fades to the panel's rounded corners, so the fade never squares them off. */
  .thread-shell { grid-area: 1 / 1; position: relative; min-height: 0; overflow: hidden; border-radius: var(--radius-panel); }
  /* The transcript fades into the surface at both ends: a short fade under the top edge, and one above the composer that reaches the surface at the composer's midpoint, so text stays readable halfway under it. */
  /* Responses run the panel's full width inside a 36px gutter. The bottom padding is the composer and the fade, so the last line scrolls clear of both. */
  .thread { width: 100%; height: 100%; margin: 0; padding: 42px 36px calc(var(--composer-height, 120px) + var(--file-chip-height, 0px) + 64px); overflow-y: auto; }
  .composer-chips { position: absolute; z-index: 5; bottom: calc(100% + 8px); left: 50%; transform: translateX(-50%); display: flex; align-items: center; gap: 6px; max-width: 100%; }
  .latest { display: grid; place-items: center; width: 32px; height: 32px; padding: 0; border: 1px solid var(--border); border-radius: var(--radius-chip); background: var(--faint); color: var(--ink); cursor: pointer; }
  .empty { color: var(--muted); text-align: center; margin-top: 18vh; }
  /* The two edges bracket the rendered window of the transcript. src/lib/transcript-window.js anchors on them. */
  .transcript-edge { display: none; }
  .user-turn { margin: 0 0 var(--turn-gap) auto; }
  .agent-turn { margin: 0 0 16px; }
  .thread-approvals { max-width: 760px; margin: 16px auto; }
  .user-message { width: fit-content; max-width: 78%; margin-left: auto; padding: 9px 13px; overflow-wrap: anywhere; background: var(--faint); border-radius: var(--radius-panel); }
  .user-message-meta.message-actions { justify-content: flex-end; align-items: center; gap: 12px; margin-top: 5px; min-height: 28px; color: var(--muted); font: var(--text-provenance)/1.45 var(--font-human); }
  .user-turn:hover .message-actions, .user-turn:focus-within .message-actions { opacity: 1; }
  .response-meta { display: flex; align-items: center; gap: 12px; margin-left: auto; color: var(--muted); font: var(--text-provenance)/1.45 var(--font-human); flex-shrink: 0; }
  .message-actions button[data-tooltip] { position: relative; }
  .message-actions button[data-tooltip]::after { content: attr(data-tooltip); position: absolute; right: 0; bottom: calc(100% + 6px); padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); white-space: nowrap; pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  .message-actions button[data-tooltip]:hover::after, .message-actions button[data-tooltip]:focus-visible::after { opacity: 1; transition-delay: 350ms; }
  .user-message > p { margin: 0; white-space: pre-wrap; }
  .missing-prompt { color: var(--muted); font: var(--text-12) var(--font-human); }
  .prompt-storage-notice { margin: 0 0 8px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .silent-note { margin: 0; color: var(--muted); font: var(--text-12) var(--font-human); }
  .resume-notice { min-height: 24px; margin: 0 0 8px; color: var(--muted); font: var(--text-12) var(--font-human); line-height: 24px; }
  .resume-notice span { position: relative; }
  .resume-notice span::after { content: attr(data-tooltip); position: absolute; z-index: 4; left: 0; bottom: calc(100% + 6px); width: max-content; max-width: 320px; padding: 6px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-human); line-height: 1.4; pointer-events: none; opacity: 0; transition: opacity 80ms ease; }
  .resume-notice span:hover::after, .resume-notice span:focus-visible::after { opacity: 1; transition-delay: 350ms; }
  .prompt-storage-notice summary { min-height: 24px; line-height: 24px; cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .prompt-storage-notice summary:hover { color: var(--ink); }
  .prompt-storage-notice p { margin: 6px 0 0; overflow-wrap: anywhere; }
  .message-attachments { display: grid; justify-items: end; gap: 4px; margin: 8px 0 0; padding: 0; list-style: none; }
  .message-attachments li { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 5px 8px; max-width: 100%; padding: 5px 8px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .message-attachments strong { flex-basis: 100%; color: var(--muted); font-weight: 400; font-size: var(--text-12); }
  .attachment-delivery-rule { margin: 4px 0 0; color: var(--muted); font: var(--text-12) var(--font-human); }
  .response { margin: 0 0 var(--reply-gap); }
  .response, .user-turn { content-visibility: auto; contain-intrinsic-size: auto 120px; }
  .streaming { position: relative; }
  .thinking { display: flex; align-items: center; gap: 9px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .tool-card { margin-top: 8px; padding: 8px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); font: var(--text-13) var(--font-mono); }
  .permission-card { color: var(--ink); }
  .permission-card strong, .applied-diff strong { font-weight: 600; }
  .permission-card p, .applied-diff p { margin: 4px 0 0; color: var(--muted); white-space: pre-wrap; overflow-wrap: anywhere; }
  .applied-diff { color: var(--ink); }
  .applied-diff summary { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-height: 24px; cursor: pointer; }
  .applied-diff summary span { color: var(--muted); }
  .applied-diff[open] summary { margin-bottom: 8px; }
  .applied-diff button { min-height: 24px; padding: 2px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: inherit; cursor: pointer; }
  .permission-plan { margin-top: 8px; padding: 8px 10px; max-height: 320px; overflow: auto; border: 1px solid var(--border); border-radius: var(--radius-control); font: var(--text-13) var(--font-human); line-height: 1.55; white-space: pre-wrap; overflow-wrap: anywhere; }
  .permission-field { display: block; width: 100%; margin-top: 8px; padding: 7px 9px; border: 1px solid var(--border); border-radius: var(--radius-control); outline: 0; background: var(--surface); color: var(--ink); font: inherit; }
  .permission-field:focus { border-color: var(--muted); }
  .permission-field::placeholder { color: var(--muted); }
  .permission-editor { min-height: 84px; resize: vertical; }
  .permission-editor-hint { color: var(--muted); font: var(--text-12) var(--font-human); }
  .permission-actions, .permission-approve-actions { display: flex; flex-wrap: wrap; gap: 6px; }
  .permission-actions { align-items: flex-start; margin-top: 8px; }
  .permission-approve-actions { justify-content: flex-end; margin-left: auto; }
  .permission-actions button { padding: 4px 8px; font: inherit; }
  .permission-card .run-error { margin-top: 6px; }
  /* One line under the reply: the receipt, then Copy at its right. */
  .receipt-line { display: flex; align-items: center; gap: 8px; margin-top: 10px; }
  .live-receipt { min-height: 28px; }
  .copy-slot { width: 32px; height: 28px; flex: none; }
  .provenance { display: flex; align-items: center; min-width: 24px; min-height: 24px; margin: 0; padding: 0; border: 0; background: transparent; color: var(--muted); font: var(--text-provenance)/1.45 var(--font-human); text-align: left; overflow-wrap: anywhere; }
  /* The line names a model and a time, so it takes the UI face with its
     proportional figures: tabular figures in that face set a decimal point
     as wide as a digit. */
  /* The receipt toggle takes the copy control's padding and hover, and its negative margin keeps the line's text on the prose edge. */
  button.provenance { padding: 4px 8px; margin-left: -8px; border-radius: var(--radius-control); }
  button.provenance:hover:not(:disabled) { background: var(--faint); color: var(--ink); }
  .receipt-marker { display: inline-block; width: 5px; height: 5px; border-right: 1px solid currentColor; border-bottom: 1px solid currentColor; transform: rotate(-45deg); transition: transform 120ms ease; vertical-align: 1px; }
  .receipt-marker.expanded { transform: rotate(45deg); }
  /* Segments sit apart on the row's gap, with no separator glyph between them. A flex row drops its whitespace-only text nodes, so the gap and not a space carries the spacing. */
  .provenance { gap: 9px; }
  .receipt-provider { display: inline-flex; flex: none; margin-right: -4px; }
  /* §1.2 permits --signal on the route segment only. */
  .provenance .route-segment { color: var(--signal); }
  .message-time { font-family: var(--font-human); font-size: var(--text-provenance); font-weight: 400; line-height: 1.45; font-variant-numeric: tabular-nums; }
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
  .receipt-record dt { overflow-wrap: anywhere; }
  .receipt-record dd { margin: 0; font-family: var(--font-mono); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
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
  .run-error { color: var(--muted); font: var(--text-12) var(--font-human); overflow-wrap: anywhere; }
  .run-status { margin: 0; color: var(--muted); font: var(--text-12) var(--font-human); }
  .cancel-error, .history-error { margin: 0 0 8px; color: var(--muted); font: var(--text-12) var(--font-human); }
  .history-error { overflow-wrap: anywhere; }
  /* The notice keeps the background service register at the composer 12px scale.
     A 15px support line would compete with the draft text. */
  .update-notice { display: grid; gap: 2px; margin: 6px 0 8px; }
  .update-notice .support { font-size: var(--text-12); }
  .run-error button { min-width: 24px; min-height: 24px; padding: 2px 6px; background: transparent; font: inherit; }
  .run-error-details { margin-top: 4px; } .run-error-details summary { width: fit-content; cursor: pointer; } .run-error-details p { margin: 4px 0 0; }
  .attachments { display: flex; flex-wrap: wrap; gap: 6px; margin: 0 -12px 8px; padding: 0 12px 8px; border-bottom: 1px solid var(--border); list-style: none; }
  .attachments li { display: inline-flex; align-items: center; gap: 5px; min-width: 0; max-width: min(260px, 100%); height: 28px; padding: 0 0 0 6px; border: 0; border-radius: var(--radius-control); background: var(--faint); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .attachments .attachment-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .attachments button.attachment-name { border: 0; background: transparent; color: var(--ink); font: inherit; padding: 0; cursor: pointer; }
  .composer-links { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; }
  .composer-links button { display: inline-flex; align-items: center; gap: 5px; max-width: 100%; color: var(--reference); border: 0; background: transparent; padding: 2px 0; font: inherit; cursor: pointer; }
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
  .composer-meta { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .composer-meta > span { flex-basis: max-content; }
  .composer-actions { display: flex; flex-wrap: wrap; justify-content: flex-end; align-items: center; gap: 6px; max-width: 100%; margin-left: auto; }
  .composer-actions button { flex-shrink: 0; white-space: nowrap; }
  .composer-icon { display: inline-flex; align-items: center; justify-content: center; min-width: 24px; min-height: 24px; padding: 4px; line-height: 0; color: var(--muted); }
  .composer-icon:hover:not(:disabled), .composer-icon[aria-pressed="true"] { color: var(--ink); background: var(--faint); }
  /* The band's one action control: an ink up arrow while the draft has text, a muted stop square in flight. */
  .composer-action { display: inline-flex; align-items: center; justify-content: center; min-width: 24px; min-height: 24px; padding: 4px; line-height: 0; }
  /* Send and Stop share one 26px box, and an empty draft keeps the box free, so Voice never moves. */
  .composer-action, .composer-action-slot { flex: none; width: 26px; height: 26px; }
  /* During a run the send button becomes Stop in the same place and color, with a filled square. */
  .composer-action.stop :global(svg) { fill: currentColor; }
  .composer-action.stop[aria-disabled="true"] { border-color: transparent; }
  .capture-status { display: flex; align-items: center; gap: 8px; font-family: var(--font-mono); }
  .capture-meter { height: 14px; display: flex; align-items: center; gap: 2px; }
  .capture-meter i { width: 2px; height: 6px; background: var(--muted); animation: capture 900ms ease-in-out infinite alternate; }
  .capture-meter i:nth-child(2), .capture-meter i:nth-child(4) { height: 10px; animation-delay: -300ms; }
  .capture-meter i:nth-child(3) { height: 14px; animation-delay: -600ms; }
  .dictation-error { margin-top: 7px; color: var(--muted); font: var(--text-12) var(--font-human); }
  .speech-install-popover { width: min(340px, 100%); padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); box-shadow: var(--shadow-overlay); }
  .speech-install-lead { margin: 7px 0 0; color: var(--ink); font: var(--text-13) var(--font-human); }
  .speech-install-lead .nowrap { white-space: nowrap; }
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
  @keyframes capture { to { transform: scaleY(.55); } }
  /* Unlike the blanket duration rule, removing this animation keeps the meter
     at its full-height resting state instead of the keyframe's 55% endpoint. */
  :global(:root[data-motion="reduce"]) .capture-meter i { animation: none; }
  :global(:root[data-motion="reduce"]) .receipt-marker { transition: none; }
</style>
