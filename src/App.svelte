<script>
  import { onMount, tick, untrack } from 'svelte'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { getCurrentWindow, UserAttentionType } from '@tauri-apps/api/window'
  import { open } from '@tauri-apps/plugin-dialog'
  import { register, unregister } from '@tauri-apps/plugin-global-shortcut'
  import { openUrl } from '@tauri-apps/plugin-opener'

  import AccessPanel from './lib/AccessPanel.svelte'
  import Settings from './lib/Settings.svelte'
  import ModelPicker from './lib/ModelPicker.svelte'
  import { currentModel, modelChipLabel } from './lib/provider-catalog.js'
  import ProviderLogo from './lib/ProviderLogo.svelte'
  import LucideIcon from './lib/LucideIcon.svelte'
  import RowControl from './lib/RowControl.svelte'
  import AssistantMarkdown from './lib/AssistantMarkdown.svelte'
  import CodeDiff from './lib/CodeDiff.svelte'
  import ConfirmDialog from './lib/ConfirmDialog.svelte'
  import Onboarding from './lib/Onboarding.svelte'
  import { commitType, readStoredType, stepType, typeSizeShortcutStep } from './lib/type-state.js'
  import { ARTIFACT_RAIL_MAX_WIDTH, ARTIFACT_RAIL_MIN_WIDTH, artifactRailShortcut, createRailController, defaultArtifactRailWidth, isArtifactRailShortcut, isRecordPanelShortcut, railBounds, recordPanelShortcut, shortcutDisplayLabel } from './lib/artifact-rail-state.js'
  import RecordPanel from './record/RecordPanel.svelte'
  import { bootState, errorState, registrationRetryState, statusState, waitingState } from './lib/auth-state.js'
  import { createBackgroundServiceNotice } from './lib/background-service-notice.js'
  import { solidMilledRingPath } from './lib/mark.js'
  import { codeDiffPermissionAnswer, composerAction, formatByteSize, permissionGateAction, permissionGateCommitHint, receiptLabel, receiptRows, receiptSummary, runAnnouncement, runFailureMessage } from './lib/chat-state.js'
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
  import { createStreamingUnderlineAction } from './lib/streaming-underline.js'
  import RunMark from './lib/RunMark.svelte'
  import { threadTitle } from './lib/thread-title.js'
  import { createVoiceGesture } from './lib/voice-gesture.js'
  import { createVoiceShortcutManager } from './lib/voice-shortcut.js'
  import { createWindowTitle } from './lib/window-title.js'

  const sealD = solidMilledRingPath()
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
  let inventoryRequestVersion = 0
  let pickerOpen = $state(false)
  let modelChip = $state()

  const composerHint = $derived(
    active?.phase === 'resuming' ? 'Reopening the existing secure session…'
      : active ? ''
        : auth.name === 'local' ? ''
          : 'Routing is automatic. Every reply carries its receipt.')

  const modelSourceLabel = $derived(modelChipLabel(inventory))
  const chipModel = $derived(currentModel(inventory))

  let settingsOpen = $state(false)
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
    else pickerOpen = true
  }

  async function chooseModel(provider, model) {
    const before = inventory
    if (inventory) inventory = { ...inventory, default_provider: provider, default_model: model }
    closePicker()
    try {
      await tauri.invoke('local_mode_set_default_model', { provider, model })
    } catch (_) {
      if (inventory && before) inventory = { ...inventory, default_provider: before.default_provider, default_model: before.default_model }
      accountStatus = 'Muniment could not save the model choice. Try again.'
    }
  }

  function openModelSettings() {
    pickerOpen = false
    openSettings('models', modelChip)
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
  let historyError = $state('')
  let historyErrorAction = $state(null)
  let threadSummaries = $state([])
  let moreThreads = $state(false)
  let loadingOlderThreads = $state(false)
  let currentThreadId = $state(null)
  let currentThreadTitle = $derived(threadSummaries.find(({ threadId }) => threadId === currentThreadId)?.title || threadTitle(messages))
  let freshThread = $state(false)
  let threadSwitching = $state(false)
  let editingThreadTitle = $state(false)
  let threadTitleDraft = $state('')
  let threadTitleInput = $state()
  let threadTitleButton = $state()
  let threadTitleBeforeEdit = ''
  let deletingThreadId = $state(null)
  let deletePending = $state(false)
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
  let recordPanelOpen = $derived(railOccupant === 'record')
  let artifactRailWidth = $state(defaultArtifactRailWidth(window.innerWidth))
  let artifactRailMaximum = $state(ARTIFACT_RAIL_MAX_WIDTH)
  let artifactRailPointer = $state()
  let recordMaximized = $state(false)
  let workspace = $state()
  let entitlementToastVisible = $state(false)
  let pairingRequests = $state([])
  let desktopClientStatus = $state(null)
  let desktopClientStatusVersion = 0
  let runtimeNotice = $state(null)
  let backgroundServiceNoticeVisible = $derived(runtimeNotice?.visible === true)
  let authRequestVersion = 0
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
  const streamingUnderline = createStreamingUnderlineAction(tick)
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

  // The thread's scroll bar thumb shows while the thread scrolls and for a
  // second after, then hides until the pointer rests on the thread.
  let threadScrolling = $state(false)
  let threadScrollTimer
  function onThreadScroll() {
    handleThreadScroll()
    threadScrolling = true
    clearTimeout(threadScrollTimer)
    threadScrollTimer = setTimeout(() => { threadScrolling = false }, 1000)
  }

  const railController = createRailController({
    readOccupant: () => railOccupant,
    onOccupant: (next) => { railOccupant = next },
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
  const { fit: fitArtifactRail, pointerDown: artifactRailPointerDown, pointerMove: artifactRailPointerMove, pointerEnd: artifactRailPointerEnd, keydown: artifactRailKeydown } = railController
  const toggleArtifactRail = () => railController.toggle('artifacts')
  const toggleRecordPanel = () => railController.toggle('record')
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

  const chatController = createChatController({
    invoke: (...args) => tauri.invoke(...args),
    listen: (...args) => window.__TAURI__?.event?.listen(...args),
    readMessages: () => messages,
    readActive: () => active,
    readAnnounced: () => announcedRun,
    readDraft: () => draft,
    readFiles: () => selectedFiles,
    readThreadId: () => currentThreadId,
    readThreadSummaries: () => threadSummaries,
    blocked: () => dictationBusy() || runtimeUpgradePending(),
    onMessages: (next) => { messages = next },
    onActive: (next) => { active = next },
    onAnnounce: (next) => { announcedRun = next },
    onDraft: (next) => { draft = next },
    onFiles: (next) => { selectedFiles = next },
    onSubmitError: (next) => { submitError = next },
    onCancelError: (next) => { cancelError = next },
    onQueueError: (next) => { queueError = next },
    onHistoryError: (next, action) => { historyError = next; historyErrorAction = action },
    onThreadSummaries: (next) => { threadSummaries = next },
    onMoreThreads: (next) => { moreThreads = next },
    onThreadSelected: (next) => { currentThreadId = next },
    onThreadSwitch: (next) => { threadSwitching = next },
    onFreshThread: (next) => { freshThread = next },
    onHistoryStart: () => { expandedReceipts = new Set(); parallelTools = new Map() },
    onHistoryLoaded: () => { pinned = true },
    onFollow: followNewContent,
    onFocus: () => tick().then(() => composer?.focus()),
    onSend: () => {},
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
    clearThreadSelection()
    void chatController.openThread(threadId)
  }

  // The open thread's row is not a button, so a modifier click alone selects it.
  function currentRowClick(event, threadId) {
    if (!(event.shiftKey || event.metaKey || event.ctrlKey)) return
    event.preventDefault()
    selectThread(threadId, event.shiftKey)
  }

  function selectThread(threadId, range) {
    const next = new Set(selectedThreadIds)
    const ids = threadSummaries.map((summary) => summary.threadId)
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
    if (count > 1) return form === 'name' ? `Delete ${count}` : `Delete ${count} threads?`
    return form === 'name' ? `Delete ${rowTitle}` : `Delete ${rowTitle}?`
  }

  function askToDeleteThread(threadId) {
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
    focusThreadRow(threadId)
  }

  // The row's actions live in a menu on right-click, Control-click or the
  // keyboard's context menu key, placed at the pointer like the native one.
  function openThreadMenu(event, threadId) {
    if (deletingThreadId === threadId) return
    event.preventDefault()
    if (selectedThreadIds.size && !selectedThreadIds.has(threadId)) clearThreadSelection()
    const row = event.currentTarget.getBoundingClientRect()
    const fromKeyboard = !event.clientX && !event.clientY
    threadMenu = { threadId, x: fromKeyboard ? row.left : event.clientX, y: fromKeyboard ? row.bottom : event.clientY }
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
    if (event.key !== 'Escape' && event.key !== 'Tab') return
    event.preventDefault()
    const threadId = threadMenu?.threadId
    closeThreadMenu()
    focusThreadRow(threadId)
  }

  function focusMenuOnMount(element) {
    element.querySelector('button')?.focus()
  }

  function menuFocusOut(event) {
    if (event.currentTarget.contains(event.relatedTarget)) return
    closeThreadMenu()
  }

  function deleteConfirmKeydown(event) {
    if (event.key !== 'Escape') return
    event.preventDefault()
    cancelDeleteThread()
  }

  async function confirmDeleteThread(threadId) {
    if (deletePending) return
    deletePending = true
    const targets = deletingThreadIds.length ? deletingThreadIds : [threadId]
    for (const id of targets) {
      if (!(await chatController.deleteThread(id))) break
    }
    deletePending = false
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

  function composerInput(event) {
    composerInputDraft = event.currentTarget.value
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
    if (!workspaceMode() || onboarding.name !== 'complete') closeRail()
  })

  function workspaceMode() {
    return auth.name === 'signed-in' || auth.name === 'local'
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
      if (version === inventoryRequestVersion) inventory = next
      return true
    } catch (_) {
      if (version === inventoryRequestVersion) {
        accountStatus = 'Muniment cannot read provider settings. Restart the app to retry.'
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
    if (localEntryPending || (auth.name !== 'signed-out' && auth.name !== 'local')) return
    settingsOpen = false
    if (auth.name === 'local') {
      try {
        await tauri.invoke('local_mode_leave')
        markerStartupLocalMode = false
      } catch (_) {
        accountStatus = 'Cloud sign-in could not start. Try again.'
        return
      }
    }
    void run('sign-in')
  }

  function startWorkspace() {
    markerStartupReady = tauri.invoke('local_mode_status')
    startupReady = markerStartupReady.then(async (active) => {
      markerStartupLocalMode = active
      if (active) {
        auth = { name: 'local', subject: null }
        void refreshInventory()
        await chatController.loadHistory()
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
      if (!connectionRecovered || markerStartupLocalMode === true) return
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
    window.__TAURI__?.event?.listen('auth-registration-retry', ({ payload }) => {
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
      entitlementToast.start()
      voiceShortcutManager.start()
    }
    const shortcuts = (event) => {
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
        const threadId = freshThread ? threadSummaries[rowPosition - 2]?.threadId : threadSummaries[rowPosition - 1]?.threadId
        if (!threadId || threadId === currentThreadId) return
        void chatController.openThread(threadId)
        return
      }
      if (workspaceMode() && onboarding.name === 'complete' && isNewThreadShortcut(event)) {
        event.preventDefault()
        void chatController.newThread()
        return
      }
      if (workspaceMode() && onboarding.name === 'complete' && isArtifactRailShortcut(event)) {
        event.preventDefault()
        toggleArtifactRail()
        return
      }
      if (workspaceMode() && onboarding.name === 'complete' && isRecordPanelShortcut(event)) {
        event.preventDefault()
        // ⌘K on a maximized record panel returns the sidebar and the thread first.
        if (recordPanelOpen && recordMaximized) toggleRecordMaximized()
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
      if (event.key === 'Escape' && railOccupant !== null) {
        event.preventDefault()
        if (recordPanelOpen && recordMaximized) toggleRecordMaximized()
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
    return active ? active.id === 'pending' || active.phase === 'resuming' : sendDisabled()
  }

  function composerActionClick() {
    if (composerActionInactive()) return
    active ? chatController.cancel() : chatController.send()
  }
</script>

<main class:onboarding-active={tauri && onboarding.name !== 'complete'}>
  {#if !workspaceMode() || onboarding.name !== 'complete'}
    <div class="lockup">
      <svg width="34" height="34" viewBox="0 0 48 48" aria-hidden="true">
        <path d={sealD} fill-rule="evenodd" />
      </svg>
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
      {#if auth.name === 'signed-out' || auth.name === 'signing-in'}
      <section class="auth-state">
        <p class="support" aria-live="polite">{auth.name === 'signing-in' ? auth.message : 'Sign in for cloud features, or use local mode.'}</p>
        {#if auth.name === 'signing-in' && auth.link}
          <a class="sign-in-link" data-testid="sign-in-link" href={auth.link} target="_blank" rel="noopener noreferrer" onclick={openSignInLink}>Open the sign-in page</a>
        {/if}
        <div class="auth-actions">
          <button class="primary" class:inactive={auth.name === 'signing-in' || localEntryPending} disabled={localEntryPending} aria-disabled={auth.name === 'signing-in' || localEntryPending ? 'true' : undefined} onclick={signIn}>Sign in</button>
          <button disabled={localEntryPending} aria-disabled={auth.name === 'signing-in' || localEntryPending ? 'true' : undefined} onclick={enterLocalMode}>Use local mode</button>
        </div>
        {#if localEntryError}<p class="record error-record" role="alert">{localEntryError}</p>{/if}
      </section>
    {:else if workspaceMode() && desktopClientStatus}
      <section class="workspace" data-testid={auth.name === 'local' ? 'local-mode' : undefined} class:macos={macOS} class:sidebar-collapsed={sidebarCollapsed} class:rail-open={railOccupant !== null} class:record-maximized={recordPanelOpen && recordMaximized} class:artifact-resizing={artifactRailPointer !== undefined} class:sidebar-resizing={sidebarPointer !== undefined} style:--artifact-rail-width={`${artifactRailWidth}px`} style:--sidebar-column={`${sidebarCollapsed ? 0 : sidebarWidth}px`} bind:this={workspace}>
        <header class="titlebar" data-tauri-drag-region>
          <div class="titlebar-sidebar" data-tauri-drag-region>
            <button type="button" class="quiet side-toggle" aria-controls="sidebar" aria-expanded={!sidebarCollapsed} aria-keyshortcuts={sidebarKeyShortcut} aria-label={`${sidebarCollapsed ? 'Expand' : 'Collapse'} sidebar`} onclick={toggleSidebar}>
              <LucideIcon name={sidebarCollapsed ? 'panel-left-open' : 'panel-left-close'} />
            </button>
            <RowControl kind="new-thread" aria-label="New thread" aria-keyshortcuts={newThreadKeyShortcut} disabled={!!active || threadSwitching} onclick={() => chatController.newThread()}>
              <LucideIcon name="plus" />
              <span>New thread</span><kbd>{shortcutDisplayLabel(newThreadKeyShortcut)}</kbd>
            </RowControl>
          </div>
          <div class="titlebar-thread" data-tauri-drag-region>
            {#if editingThreadTitle}
              <input class="thread-title" aria-label="Thread name" maxlength="160" bind:this={threadTitleInput} value={threadTitleDraft} oninput={limitThreadTitle} onkeydown={threadTitleKeydown} onblur={commitThreadTitle}>
            {:else}
              <h1 class="thread-title-heading" aria-label={currentThreadTitle} data-tauri-drag-region><RowControl kind="thread-title" aria-label="Rename thread" disabled={!currentThreadId} bind:element={threadTitleButton} onclick={() => editThreadTitle(currentThreadTitle)} onkeydown={threadTitleButtonKeydown}>{currentThreadTitle}</RowControl></h1>
            {/if}
            <span class="title-spacer" data-tauri-drag-region></span>
            <span class="update-slot" data-tauri-drag-region aria-hidden="true"></span>
            <RowControl kind="artifacts-toggle" aria-controls="artifact-rail" aria-expanded={artifactRailOpen} aria-keyshortcuts={artifactShortcut} aria-label={`${artifactRailOpen ? 'Close' : 'Open'} artifact rail`} onclick={toggleArtifactRail}>Artifacts <kbd>{shortcutDisplayLabel(artifactShortcut)}</kbd></RowControl>
            <RowControl kind="record-toggle" aria-controls="record-panel" aria-expanded={recordPanelOpen} aria-keyshortcuts={recordShortcut} aria-label={`${recordPanelOpen ? 'Close' : 'Open'} record panel`} onclick={toggleRecordPanel}>Record <kbd>{shortcutDisplayLabel(recordShortcut)}</kbd></RowControl>
          </div>
        </header>
        <aside id="sidebar" class="sidebar">
          {#if !sidebarCollapsed}
            <h2 id="thread-list-title" class="side-label">Threads</h2>
            <div class="side-scroll">
            <h3 class="side-group">Untitled</h3>
            <ul class="thread-list" aria-labelledby="thread-list-title">
              {#if freshThread}
                <li class="thread-row active-thread" data-fresh-thread aria-current="true" aria-keyshortcuts={threadRowShortcut(1)}><span></span><div class="thread-row-title">{currentThreadTitle}</div></li>
              {/if}
              {#each threadSummaries as summary, index (summary.threadId)}
                {@const title = summary.title || 'New thread'}
                {@const current = !freshThread && summary.threadId === (currentThreadId ?? threadSummaries[0]?.threadId)}
                {@const rowTitle = current ? summary.title || currentThreadTitle : title}
                {@const rowPosition = index + 1 + (freshThread ? 1 : 0)}
                {@const selected = selectedThreadIds.has(summary.threadId)}
                <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                <li class="thread-record" class:selected oncontextmenu={(event) => openThreadMenu(event, summary.threadId)}>
                  {#if current}
                    <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
                    <div class="thread-row active-thread" class:selected data-thread-id={summary.threadId} aria-current="true" aria-keyshortcuts={rowPosition <= 9 ? threadRowShortcut(rowPosition) : undefined} onclick={(event) => currentRowClick(event, summary.threadId)}><span></span><div class="thread-row-title">{rowTitle}</div><time datetime={summary.updatedAt}>{relativeTime(summary.updatedAt)}</time></div>
                  {:else}
                    <button class="thread-row" class:selected data-thread-id={summary.threadId} data-selected={selected ? 'true' : undefined} aria-keyshortcuts={rowPosition <= 9 ? threadRowShortcut(rowPosition) : undefined} aria-disabled={active ? 'true' : undefined} onclick={(event) => threadRowClick(event, summary.threadId)} onkeydown={(event) => threadRowKeydown(event, summary.threadId)}><span></span><div class="thread-row-title">{rowTitle}</div><time datetime={summary.updatedAt}>{relativeTime(summary.updatedAt)}</time></button>
                  {/if}
                  {#if deletingThreadId !== summary.threadId}
                    <button type="button" class="quiet thread-delete" aria-label={deleteLabel(summary.threadId, rowTitle)} disabled={!!active || threadSwitching} onclick={() => askToDeleteThread(summary.threadId)}><LucideIcon name="trash-2" variant="action" size={14} /></button>
                  {/if}
                  {#if deletingThreadId === summary.threadId}
                    <div class="thread-delete-confirm" role="group" aria-label={deleteLabel(summary.threadId, rowTitle, 'question')}>
                      <button type="button" disabled={deletePending} onclick={() => confirmDeleteThread(summary.threadId)} onkeydown={deleteConfirmKeydown}>{deletingThreadIds.length > 1 ? deleteLabel(summary.threadId, rowTitle) : 'Delete'}</button>
                      <button type="button" disabled={deletePending} onclick={cancelDeleteThread} onkeydown={deleteConfirmKeydown}>Cancel</button>
                    </div>
                  {/if}
                  {#if threadMenu?.threadId === summary.threadId}
                    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                    <div class="thread-menu" role="menu" aria-label={`${rowTitle} actions`} style:left={`${threadMenu.x}px`} style:top={`${threadMenu.y}px`} onkeydown={threadMenuKeydown} onfocusout={menuFocusOut} use:focusMenuOnMount>
                      <button type="button" role="menuitem" aria-label={deleteLabel(summary.threadId, rowTitle)} disabled={!!active || threadSwitching} onclick={() => askToDeleteThread(summary.threadId)}>{deletionTargets(summary.threadId).length > 1 ? deleteLabel(summary.threadId, rowTitle) : 'Delete'}</button>
                    </div>
                  {/if}
                </li>
              {/each}
            </ul>
            {#if moreThreads}
              <button type="button" class="older-threads" disabled={loadingOlderThreads} onclick={loadOlderThreads}>Older threads</button>
            {/if}
            </div>
          {/if}
          {#if !sidebarCollapsed}
          <div class="side-foot">
            <div class="settings-block">
              <button class="side-action" aria-haspopup="dialog" aria-expanded={settingsOpen} aria-keyshortcuts={settingsKeyShortcut} bind:this={settingsButton} onclick={toggleSettings}><LucideIcon name="settings" size={18} /><span>Settings</span></button>
            </div>
            {#if auth.name === 'signed-in'}
              <AccessPanel {tauri} subject={auth.subject} onSignOut={() => run('sign-out')} />
            {:else}
              <button class="side-action" disabled={!!active || localEntryPending} onclick={signIn}><LucideIcon name="log-in" size={18} /><span>Sign in to cloud</span></button>
            {/if}
          </div>
          {/if}
        </aside>
        {#if !sidebarCollapsed}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <div
            class="sidebar-divider"
            role="separator"
            aria-labelledby="thread-list-title"
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
        <div class="thread-panel" style:--composer-height="{composerBoxHeight}px">
        {#if draggingFiles}<div class="drop-affordance" role="status"><strong>Drop files to add them</strong><span>Saved locally · supported images sent with first prompt</span></div>{/if}
        <div class="thread-shell">
        <div class="thread" class:scrolling={threadScrolling} role="region" aria-label={`Transcript: ${currentThreadTitle}`} bind:this={thread} onscroll={onThreadScroll}>
          {#if historyError}<p class="history-error" role="alert">{historyError} {#if historyErrorAction}<button onclick={historyErrorAction.run}>{historyErrorAction.label}</button>{/if}</p>{/if}
          {#if messages.length === 0}<p class="empty">{auth.name === 'local' ? 'Your model answers here. Ask anything.' : "Ask anything. Your org's routing decides which model answers."}</p>{/if}
          {#each messages as message}
            {#if message.role === 'user'}
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
              </div>
            {:else}
            <div class="response">
              {#if message.run.promptStorageNotice}
                <details class="prompt-storage-notice">
                  <summary>Prompt text stays in runtime memory for this run.</summary>
                  <p>{message.run.promptStorageNotice}</p>
                </details>
              {/if}
              <!-- One mark for the whole run in flight, in its own block so its exit holds nothing else back. -->
              {#if message.run.phase === 'thinking' || message.run.phase === 'streaming'}<RunMark stage={message.run.stage} />{/if}
              {#if message.run.phase === 'acquiring-pi'}
                <p class="thinking">{runAnnouncement(message.run)}</p>
              {:else if message.run.phase === 'thinking'}
              {:else if message.run.phase === 'streaming'}<div class="streaming" use:streamingUnderline={message.run.text}><AssistantMarkdown text={message.run.text} caret /><span class="streaming-rule" aria-hidden="true"></span></div>
              {:else}<AssistantMarkdown text={message.run.text} />{/if}
              {#each message.run.appliedDiffs ?? [] as appliedDiff}
                <div class="applied-diff tool-card">
                  <strong>Applied file changes</strong>
                  {#if appliedDiff.diff}<CodeDiff codeDiff={appliedDiff.diff} />
                  {:else}<p>The changes were applied, but their record is no longer stored.</p>{/if}
                </div>
              {/each}
              {#if message.run.phase === 'failed'}<div class="run-error">{runFailureMessage(message.run)} <button disabled={!message.run.prompt?.trim() || !!active || dictationBusy() || runtimeUpgradePending()} onclick={() => { draft = message.run.prompt; chatController.send() }}>Try again</button></div>{/if}
              {#if message.run.phase === 'cancelled'}<div class="run-error">Reply stopped. {#if message.run.prompt}<button disabled={dictationBusy() || runtimeUpgradePending()} onclick={() => { draft = message.run.prompt; chatController.send() }}>Try again</button>{/if}</div>{/if}
              {#if message.run.phase === 'interrupted'}<div class="run-error" role={message.run.resumeError ? 'alert' : undefined}>{message.run.resumeError ?? 'Reply interrupted.'} {#if message.run.resumable}<button disabled={!!active || dictationBusy() || runtimeUpgradePending()} onclick={() => chatController.resume(message.run)}>Resume</button>{:else if message.run.prompt}<button disabled={dictationBusy() || runtimeUpgradePending()} onclick={() => { draft = message.run.prompt; chatController.send() }}>Try again</button>{/if}</div>{/if}
              {#if message.run.phase === 'pending-permission' && message.run.pendingPermission}
                {@const gate = message.run.pendingPermission}
                {@const answerState = permissionState(message.run)}
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
              {#if message.run.phase === 'complete'}
                {@const summary = receiptSummary(message.run.receipt)}
                {@const rows = receiptRows(message.run.receipt, message.run.recalls)}
                {@const recorded = summary.route !== null || summary.model !== null || summary.time !== null || rows.length > 0}
                {@const expanded = rows.length > 0 && expandedReceipts.has(message.run.id)}
                {@const failure = copyFailure(copy, message.run.id, modifierLabel)}
                <!-- One line: the receipt, then §3.2's action row at its right, copy only in this slice. -->
                <div class="receipt-line">
                  {#if recorded}
                    {#if rows.length > 0}
                      <button class="provenance" aria-expanded={expanded} aria-label={`${expanded ? 'Collapse' : 'Expand'} receipt: ${receiptLabel(message.run.receipt)}`} onclick={() => toggleReceipt(message.run.id)}><span class:expanded class="receipt-marker" aria-hidden="true"></span>{#if summary.route !== null}<span class="route-segment">{summary.route}</span>{/if}{#if summary.model !== null}{#if summary.route !== null}{' '}<span aria-hidden="true">→</span>{' '}{/if}<span>{summary.model}</span>{/if}{#if summary.time !== null}{#if summary.route !== null || summary.model !== null}{' '}{/if}<span class="receipt-time"><LucideIcon name="clock" variant="action" size={12} />{summary.time}</span>{/if}</button>
                    {:else}
                      <!-- One row adds nothing beyond the line: a clock stands where the chevron would, and nothing expands. -->
                      <p class="provenance" aria-label={`Receipt: ${receiptLabel(message.run.receipt)}`}>{#if summary.route !== null}<span class="route-segment">{summary.route}</span>{/if}{#if summary.model !== null}{#if summary.route !== null}{' '}<span aria-hidden="true">→</span>{' '}{/if}<span>{summary.model}</span>{/if}{#if summary.time !== null}{#if summary.route !== null || summary.model !== null}{' '}{/if}<span class="receipt-time"><LucideIcon name="clock" variant="action" size={12} />{summary.time}</span>{/if}</p>
                    {/if}
                  {:else}
                    <p class="provenance">Receipt unavailable</p>
                  {/if}
                  <div class="message-actions">
                    <button type="button" onclick={() => copyResponse(message.run)}>{#if copyConfirmed(copy, message.run.id)}<LucideIcon name="check" variant="action" size={14} />{:else}<LucideIcon name="copy" variant="action" size={14} />{/if}{copyLabel(copy, message.run.id)}</button>
                  </div>
                </div>
                {#if expanded}
                  <dl class="receipt-record">
                    {#each rows as row}
                      <div><dt>{row.label}</dt><dd class:route-value={row.route}>{row.value}{#each row.files ?? [] as file}<span class="recall-file">{file}</span>{/each}</dd></div>
                    {/each}
                  </dl>
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
        <div class="composer" bind:this={composerBox}>
        {#if pickerOpen}
          <ModelPicker {inventory} current={currentModel(inventory)} onchoose={chooseModel} onmanage={openModelSettings} onclose={closePicker} />
        {/if}
        {#if dictation.state === 'modelNotInstalled' && !speechInstallDismissed}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <section class="speech-install-popover" aria-labelledby="speech-install-title" onkeydown={speechInstallKeydown}>
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
          {#if selectedFiles.length}
            <ul class="attachments" aria-label="Selected files">
              {#each selectedFiles as file}
                <li><span>{file.displayName}</span><span>{formatByteSize(file.byteLength)}</span><button type="button" aria-label={`Remove ${file.displayName}`} onclick={() => { selectedFiles = selectedFiles.filter(({ path }) => path !== file.path) }}>Remove</button></li>
              {/each}
            </ul>
          {/if}
          <div class="composer-input">
            <label class="visually-hidden" for="composer-message">Message</label>
            <textarea id="composer-message" aria-describedby={threadSwitching || isDictationActive(dictation) || composerHint ? 'composer-hint' : undefined} bind:this={composer} use:focusComposerOnMount bind:value={draft} oninput={composerInput} onkeydown={keydown} rows="2" placeholder={active?.phase === 'resuming' ? 'Resuming interrupted reply…' : 'Ask anything'} disabled={composer && (active?.phase === 'resuming' || threadSwitching)}></textarea>
          </div>
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
            {#if !active}<button type="button" class="quiet composer-icon" aria-label="Add files" onclick={chooseFiles}><LucideIcon name="plus" variant="action" size={16} /></button>{/if}
            {#if auth.name === 'local'}
              <button type="button" class="quiet model-chip" bind:this={modelChip} aria-haspopup="dialog" aria-expanded={pickerOpen} onclick={togglePicker}>{#if chipModel}<ProviderLogo provider={chipModel.provider} size={14} />{/if}<span class="model-chip-label">{modelSourceLabel}</span><LucideIcon name="chevron-down" variant="action" size={12} /></button>
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
        </div>
        </div>
        {#if entitlementToastVisible}
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
          <aside id="artifact-rail" class="artifact-rail" aria-labelledby="artifact-rail-title">
            <header>
              <h2 id="artifact-rail-title">Artifacts</h2>
            </header>
            <div class="artifact-empty">
              <p>No artifacts yet</p>
            </div>
          </aside>
        {/if}
        {#if recordPanelOpen}
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
          <RecordPanel {tauri} maximized={recordMaximized} ontogglemaximized={toggleRecordMaximized} onask={askAboutView} />
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

  {#if settingsOpen}
    <Settings {tauri} bind:section={settingsSection} onclose={closeSettings} homePath={onboarding.homePath} onchangehome={openHomeSettings} local={auth.name === 'local'} signInDisabled={!!active || localEntryPending} onsignin={signIn} {accountStatus} {inventory} oninventory={(next) => { inventory = next }} voiceShortcut={globalVoiceShortcutValue} voiceShortcutChanging={globalVoiceChanging} onVoiceShortcutChange={changeVoiceShortcut} defaultVoiceShortcut={holdToTalkShortcut()} />
  {/if}
{#if pairingRequests[0]}
  {#key pairingRequests[0]}
    <ConfirmDialog title="Approve Muniment connection" onDecision={decidePairing}>
      <p>The connecting program supplied these claims: kind {pairingRequests[0].claimedKind} and version {pairingRequests[0].claimedVersion}. Allow this program to access workspace {pairingRequests[0].workspace} with the scopes {pairingRequests[0].scopes.join(' and ')}?</p>
    </ConfirmDialog>
  {/key}
{/if}

<style>
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

  /* Lockup (§1.8): the seal, static ink at rest beside the wordmark,
     Schibsted 600, lowercase, −1% tracking. */
  .lockup {
    display: flex;
    align-items: center;
    gap: 13px;
  }

  .lockup path {
    fill: var(--ink);
  }

  .name {
    font-size: var(--text-28);
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
  .workspace.artifact-resizing, .workspace.sidebar-resizing { transition: none; }
  /* The sidebar yields frame space at the window minimum while the thread keeps 320px. */
  .workspace.rail-open { grid-template-columns: minmax(0, var(--sidebar-column)) minmax(320px, 1fr) var(--artifact-rail-width); grid-template-areas: "title title title" "side thread rail"; }
  /* A maximized record takes the whole frame; the sidebar and the thread stay mounted and hidden. */
  .workspace.record-maximized { grid-template-columns: minmax(0, 1fr); grid-template-areas: "title" "rail"; }
  .workspace.record-maximized .sidebar, .workspace.record-maximized .thread-panel, .workspace.record-maximized .sidebar-divider { display: none; }
  .sidebar, .thread-panel, .artifact-rail { min-height: 0; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); }
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
     last light ends at 69pt. New thread ends
     at 251px in the installed app and at 252px in the probe's Chromium, and the
     thread title starts one gap and a rounding pixel later. */
  .workspace.macos { --titlebar-height: 30px; --titlebar-inset: 78px; --titlebar-controls-end: 272px; transition: --sidebar-column 180ms ease; }
  .workspace.macos.artifact-resizing, .workspace.macos.sidebar-resizing { transition: none; }
  .workspace:not(.macos) .titlebar-sidebar, .workspace:not(.macos) .titlebar-thread { display: contents; }
  /* The title row is a subgrid with no margin and no padding of its own: padding
     on a subgrid shifts its tracks past the frame in WebKit, which pushed the
     artifact control off the window. The native clearance is the sidebar
     part's padding, so both parts track their panel columns in every engine. */
  .workspace.macos .titlebar { display: grid; grid-template-columns: subgrid; margin: 0; padding: 0; }
  /* The sidebar part is at least as wide as its controls, so a narrow sidebar column never hides New thread. */
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
  .thread-title-heading { display: flex; min-width: 24px; max-width: 100%; margin: 0; font: inherit; }
  /* The rename field keeps the title control's register while it shows. */
  /* Editing is the rename control's active state: the composer's muted hairline, no ring. */
  input.thread-title { flex: 0 1 320px; max-width: 100%; overflow: hidden; border: 1px solid var(--muted); outline: 0; background: transparent; color: var(--ink); font: inherit; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; user-select: text; }
  kbd { margin-left: 10px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .title-spacer { flex: 1; align-self: stretch; min-width: 24px; }
  .sidebar { grid-area: side; min-width: 0; display: flex; flex-direction: column; padding: 0; }
  .side-scroll { flex: 1; min-height: 0; padding: 4px 6px 8px; overflow-y: auto; }
  .side-group { margin: 6px 8px 2px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  /* Clip labels during the panel slide without clipping the profile popover. */
  .side-label, .older-threads, .side-action span { overflow: hidden; }
  .side-toggle { line-height: 0; }
  .side-toggle:hover:not(:disabled) { border-color: transparent; background: var(--faint); }
  .side-toggle:hover:not(:disabled) :global(.side-icon) { color: var(--ink); }
  .side-action, .thread-row { width: 100%; display: flex; align-items: center; gap: 8px; min-height: 28px; padding: 4px 8px; border-color: transparent; background: transparent; text-align: left; }
  .thread-list { padding: 0; list-style: none; }
  .older-threads { width: 100%; margin-top: 4px; border-color: transparent; background: transparent; color: var(--muted); }
  .thread-record { position: relative; }
  .thread-row { font: inherit; font-size: var(--text-13); color: var(--ink); border: 1px solid transparent; border-radius: var(--radius-control); user-select: none; -webkit-user-select: none; }
  button.thread-row:hover:not([aria-disabled="true"]) { background: var(--faint); }
  /* A selected row reads darker than the open thread's faint row, so a selection and the open thread never look alike. */
  .thread-row.selected { background: color-mix(in srgb, var(--ink) 14%, var(--surface)); }
  /* The row's delete control shows while the pointer or focus rests on the row, in the time's place. */
  .thread-delete { position: absolute; top: 50%; right: 4px; min-width: 22px; min-height: 22px; padding: 0 4px; transform: translateY(-50%); color: var(--muted); opacity: 0; pointer-events: none; }
  .thread-record:hover .thread-delete, .thread-record:focus-within .thread-delete { opacity: 1; pointer-events: auto; }
  .thread-record:hover .thread-row time, .thread-record:focus-within .thread-row time { visibility: hidden; }
  .thread-delete:hover:not(:disabled) { color: var(--ink); background: var(--faint); }
  button.thread-row[aria-disabled="true"] { opacity: .55; }
  /* The time always shows: the title takes the rest of the row and fades at its end. */
  .thread-row time { flex: none; margin-left: auto; color: var(--muted); font: var(--text-provenance) var(--font-mono); white-space: nowrap; }
  .thread-row > span { flex: 0 0 5px; }
  .thread-row-title { flex: 1 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; mask-image: linear-gradient(to right, currentColor calc(100% - 28px), transparent); }
  .thread-menu { position: fixed; z-index: 4; min-width: 120px; padding: 4px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); box-shadow: var(--shadow-overlay); }
  .thread-menu button { width: 100%; min-width: 24px; min-height: 24px; padding: 3px 8px; border-color: transparent; background: transparent; color: var(--ink); font-size: var(--text-13); text-align: left; }
  .thread-menu button:hover:not(:disabled) { border-color: transparent; background: var(--faint); }
  .thread-delete-confirm { position: absolute; inset: 0; display: flex; align-items: center; justify-content: flex-end; gap: 5px; min-width: 0; padding: 5px 7px; border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); }
  .thread-delete-confirm button { flex: none; min-width: 24px; min-height: 24px; padding: 3px 6px; border-color: transparent; background: transparent; color: var(--ink); font: inherit; }
  /* A narrow sidebar shortens the first control, never its start. */
  .thread-delete-confirm button:first-child { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: left; }
  .thread-delete-confirm button:hover:not(:disabled) { background: var(--faint); }
  .side-action span { flex: 1; min-width: 0; }
  /* The foot of the sidebar: Settings above the account row, under one edge-to-edge hairline. */
  .side-foot { margin-top: auto; padding: 4px 6px 6px; border-top: 1px solid var(--border); }
  /* Settings is its own group: one hairline under it, above the sign-in button or the account panel. */
  .settings-block { padding: 0 0 4px; margin-bottom: 4px; border-bottom: 1px solid var(--border); }
  .side-foot :global(.profile-block) { margin-top: 0; padding-top: 0; border-top: 0; }
  .side-foot :global(.profile-button) { min-height: 28px; padding: 4px 8px; }
  /* The panel is one popup over the thread, never a second settings surface. */
  /* Collapsed means gone: the column is zero wide, the empty panel drops its hairline and padding for the slide, and the thread panel takes the gap. */
  .workspace.sidebar-collapsed .sidebar { padding: 0; border-width: 0; overflow: hidden; }
  .workspace.sidebar-collapsed .thread-panel { margin-left: calc(-1 * var(--frame-width)); }
  .side-label { flex: none; margin: 0; padding: 10px 14px 8px; border-bottom: 1px solid var(--border); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .active-thread { background: var(--faint); }
  /* §1.2 forbids signal on selection states; the mockup's current-thread dot is ink. */
  .active-thread > span { width: 5px; height: 5px; border-radius: 50%; background: var(--ink); }
  .quiet { background: transparent; border-color: transparent; }
  /* Both dividers sit over the panel gap and draw nothing while hovered or dragged. */
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
  .workspace.sidebar-resizing .thread-panel { transition: none; }
  .thread-shell { grid-area: 1 / 1; position: relative; min-height: 0; }
  /* The transcript fades into the surface at both ends: a short fade under the top edge, and one above the composer that reaches the surface at the composer's midpoint, so text stays readable halfway under it. */
  .thread-shell::before { content: ''; position: absolute; left: 0; right: 0; top: 0; height: 48px; background: linear-gradient(to bottom, var(--surface), transparent); pointer-events: none; }
  .thread-shell::after { content: ''; position: absolute; left: 0; right: 0; bottom: 0; height: calc(var(--composer-height, 120px) + 72px); background: linear-gradient(to bottom, transparent, var(--surface) calc(var(--composer-height, 120px) / 2 + 48px)); pointer-events: none; }
  /* Responses run the panel's full width inside a 36px gutter. The bottom padding is the composer and the fade, so the last line scrolls clear of both. */
  .thread { width: 100%; height: 100%; margin: 0; padding: 42px 36px calc(var(--composer-height, 120px) + 64px); overflow-y: auto; }
  .thread.scrolling::-webkit-scrollbar-thumb { background: var(--border); }
  .latest { position: absolute; z-index: 2; left: 50%; bottom: calc(var(--composer-height, 120px) + 38px); transform: translateX(-50%); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); font: var(--text-12) var(--font-mono); box-shadow: var(--shadow-overlay); }
  .empty { color: var(--muted); text-align: center; margin-top: 18vh; }
  .user-turn { margin: 0 0 28px auto; }
  .user-message { width: fit-content; max-width: 78%; margin-left: auto; padding: 9px 13px; overflow-wrap: anywhere; background: var(--faint); border-radius: var(--radius-panel); }
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
  .response { margin: 0 0 34px; }
  .streaming { position: relative; }
  .streaming-rule { position: absolute; height: 2px; background: var(--signal); pointer-events: none; }
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
  .receipt-time { display: inline-flex; align-items: center; gap: 4px; }
  /* The expanded receipt sits plain under the provenance line: no box. */
  .receipt-record { display: grid; row-gap: 6px; box-sizing: border-box; width: min(100%, 560px); margin: 8px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .receipt-record div { display: grid; grid-template-columns: 88px minmax(0, 1fr); gap: 12px; }
  .receipt-record dd { margin: 0; font-family: var(--font-mono); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .receipt-record .recall-file { display: block; }
  .receipt-record .route-value { color: var(--signal); }
  /* §3.2: hover or focus reveals the row. Only opacity carries the reveal. The row
     always holds its space, so nothing reflows and nothing is ever obscured, and the
     button keeps its place in the tab order. `visibility: hidden` would strip it from
     that order exactly as `display: none` does, which would make focus unreachable
     and the :focus-within reveal below unreachable with it. */
  .message-actions { display: flex; gap: 2px; opacity: 0; transition: opacity 120ms ease; }
  .response:hover .message-actions, .response:focus-within .message-actions { opacity: 1; }
  .message-actions button { display: inline-flex; align-items: center; gap: 5px; padding: 4px 8px; border-color: transparent; background: transparent; color: var(--muted); font-size: var(--text-12); }
  .message-actions button:hover:not(:disabled) { border-color: transparent; background: var(--faint); color: var(--ink); }
  /* §1.2: focus rings are ink, never signal. */
  .message-actions button:disabled { opacity: .45; }
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
  .composer { grid-area: 1 / 1; align-self: end; z-index: 1; position: relative; width: min(760px, calc(100% - 48px)); margin: 0 auto 24px; padding: 12px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .composer:focus-within { border-color: var(--muted); }
  .attachments { display: flex; flex-wrap: wrap; gap: 6px; margin: 0 -12px 8px; padding: 0 12px 8px; border-bottom: 1px solid var(--border); list-style: none; }
  .attachments li { display: flex; align-items: center; gap: 6px; max-width: 100%; padding: 4px 6px 4px 9px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .attachments span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .attachments button { padding: 1px 5px; border: 0; background: transparent; color: inherit; font-size: var(--text-12); }
  .composer-input { position: relative; }
  /* No padding and no border: the composer supplies both, so the measured
     scrollHeight is pure text and the overlay lands on the same grid. */
  textarea { display: block; width: 100%; resize: none; padding: 0; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; }
  /* The input no longer keeps a spare empty row once it grows, so the action
     row carries the gap itself, matching the owner mockup's 8px .comprow rhythm. */
  .composer-row { display: flex; flex-wrap: wrap; justify-content: space-between; align-items: center; gap: 8px; margin-top: 8px; color: var(--muted); font-size: var(--text-12); }
  .composer-meta { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .composer-meta > span { flex-basis: max-content; }
  /* The three composer controls share the plus button's box: 4px padding, a 24px minimum, the control radius. */
  .model-chip { flex: none; display: inline-flex; align-items: center; gap: 5px; min-width: 24px; min-height: 24px; padding: 3px 4px; border: 1px solid transparent; border-radius: var(--radius-control); color: var(--ink); font: var(--text-12) var(--font-mono); white-space: nowrap; }
  .model-chip:hover:not(:disabled) { background: var(--faint); }
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
  .speech-install-popover { position: absolute; z-index: 5; right: 0; bottom: calc(100% + 8px); width: min(340px, 100%); padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-panel); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); box-shadow: var(--shadow-overlay); }
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
