<script>
  import { onMount, tick, untrack } from 'svelte'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { getCurrentWindow, UserAttentionType } from '@tauri-apps/api/window'
  import { open } from '@tauri-apps/plugin-dialog'
  import { register, unregister } from '@tauri-apps/plugin-global-shortcut'

  import AccessPanel from './lib/AccessPanel.svelte'
  import AssistantMarkdown from './lib/AssistantMarkdown.svelte'
  import CodeDiff from './lib/CodeDiff.svelte'
  import ConfirmDialog from './lib/ConfirmDialog.svelte'
  import Onboarding from './lib/Onboarding.svelte'
  import { ARTIFACT_RAIL_MAX_WIDTH, ARTIFACT_RAIL_MIN_WIDTH, artifactRailShortcut, createArtifactRailController, defaultArtifactRailWidth, isArtifactRailShortcut, shortcutDisplayLabel } from './lib/artifact-rail-state.js'
  import { bootState, errorState, registrationRetryState, statusState, waitingState } from './lib/auth-state.js'
  import { createBackgroundServiceNotice } from './lib/background-service-notice.js'
  import { ringPath, solidMilledRingPath } from './lib/mark.js'
  import { codeDiffPermissionAnswer, composerAction, formatByteSize, permissionGateAction, permissionGateCommitHint, receiptLabel, receiptRows, receiptSummary, runAnnouncement, toolName, toolStatus } from './lib/chat-state.js'
  import { createChatController } from './lib/chat-controller.js'
  import { composerHeight } from './lib/composer-size.js'
  import { createDictationController } from './lib/dictation-controller.js'
  import { ariaKeyShortcut, holdToTalkShortcut, isDictationActive } from './lib/dictation-state.js'
  import { createEntitlementToast } from './lib/entitlement-toast.js'
  import { createChatTranscriptController } from './lib/chat-transcript-controller.js'
  import { copyAnnouncement, copyConfirmed, copyFailure, copyLabel } from './lib/message-actions.js'
  import { onboardingLoadingState, onboardingSettingsState } from './lib/onboarding-state.js'
  import { relativeTime } from './lib/relative-time.js'
  import { SIDEBAR_STORAGE_KEY, isNewThreadShortcut, isSidebarShortcut, newThreadShortcut, serializeSidebarCollapsed, sidebarShortcut, storedSidebarCollapsed, threadRowShortcut, threadRowShortcutPosition } from './lib/sidebar-state.js'
  import { formatBytes, installStateWords } from './lib/speech-install.js'
  import { createStreamingUnderlineAction } from './lib/streaming-underline.js'
  import { thinkingSettle } from './lib/thinking-transition.js'
  import { threadTitle } from './lib/thread-title.js'
  import { createVoiceGesture } from './lib/voice-gesture.js'
  import { createVoiceShortcutManager } from './lib/voice-shortcut.js'
  import { createWindowTitle } from './lib/window-title.js'

  const markD = ringPath()
  const thinkingMarkD = solidMilledRingPath()
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

  function fullDateTime(timestamp) {
    const date = new Date(timestamp)
    return Number.isNaN(date.getTime()) ? '' : date.toLocaleString()
  }

  const tauri = window.__TAURI__?.core
  let auth = $state(bootState)
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
  let globalVoiceRegistered = false
  let globalVoiceError = $state(false)
  let globalVoiceShortcutValue = $state(holdToTalkShortcut())
  let globalVoiceChanging = $state(true)
  let composer = $state()
  let composerRow = $state()
  let composerInputDraft
  let wasInWorkspace = false
  let onboarding = $state(onboardingLoadingState)
  let artifactRailOpen = $state(false)
  let artifactRailWidth = $state(defaultArtifactRailWidth(window.innerWidth))
  let artifactRailMaximum = $state(ARTIFACT_RAIL_MAX_WIDTH)
  let artifactRailPointer = $state()
  let workspace = $state()
  let entitlementToastVisible = $state(false)
  let pairingRequests = $state([])
  let desktopClientStatus = $state(null)
  let desktopClientStatusVersion = 0
  let backgroundServiceNoticeVisible = $state(false)
  let authRequestVersion = 0
  const artifactShortcut = artifactRailShortcut()
  let destroyed = false
  const sidebarWidth = 260
  const sidebarRailWidth = 52
  const minimumThreadWidth = 320
  let sidebarCollapsed = $state(storedSidebarCollapsed())
  const sidebarKeyShortcut = sidebarShortcut()
  const newThreadKeyShortcut = newThreadShortcut()
  const modifierLabel = shortcutDisplayLabel(sidebarKeyShortcut).slice(0, -1)
  const sidebarHint = `${modifierLabel}\\`
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

  const artifactRailController = createArtifactRailController({
    readOpen: () => artifactRailOpen,
    readWidth: () => artifactRailWidth,
    readMaximum: () => artifactRailMaximum,
    readPointer: () => artifactRailPointer,
    readAvailableWidth: availableArtifactRailWidth,
    readRightEdge: () => workspace?.getBoundingClientRect().right || window.innerWidth,
    readViewportWidth: () => workspace?.clientWidth || window.innerWidth,
    onOpen: (next) => { artifactRailOpen = next },
    onWidth: (next) => { artifactRailWidth = next },
    onMaximum: (next) => { artifactRailMaximum = next },
    onPointer: (next) => { artifactRailPointer = next },
  })
  const { fit: fitArtifactRail, toggle: toggleArtifactRail, pointerDown: artifactRailPointerDown, pointerMove: artifactRailPointerMove, pointerEnd: artifactRailPointerEnd, keydown: artifactRailKeydown } = artifactRailController

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
    onVisible: (visible) => { backgroundServiceNoticeVisible = visible },
  })

  // The notice decides its own visibility, so every status reaches it beside
  // the field the rest of the shell reads. Both status paths call this one
  // function, so the listener and the poll act alike.
  function applyDesktopClientStatus(status) {
    // The runtime drops a chat-event subscriber whose queue fills, and the
    // desktop resubscribes after a retry. No window saw the events inside that
    // gap, so the shell reads the open thread again on the recovery. The first
    // status compares against no earlier status, so it re-reads nothing.
    const chatEventsRecovered = desktopClientStatus?.chat_events_connected === false
      && status?.chat_events_connected === true
    desktopClientStatus = status
    backgroundServiceNotice.update(status)
    if (chatEventsRecovered) void chatController.refreshOpenThread()
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
    editThreadTitle(event.currentTarget.title)
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

  function askToDeleteThread(threadId) {
    deletingThreadId = threadId
  }

  function cancelDeleteThread() {
    const threadId = deletingThreadId
    deletingThreadId = null
    void tick().then(() => {
      Array.from(document.querySelectorAll('[data-delete-thread]'))
        .find((button) => button.dataset.deleteThread === threadId)?.focus()
    })
  }

  function deleteConfirmKeydown(event) {
    if (event.key !== 'Escape') return
    event.preventDefault()
    cancelDeleteThread()
  }

  async function confirmDeleteThread(threadId) {
    if (deletePending) return
    deletePending = true
    await chatController.deleteThread(threadId)
    deletePending = false
    deletingThreadId = null
  }

  function availableArtifactRailWidth() {
    return Math.max(ARTIFACT_RAIL_MIN_WIDTH, Math.min(ARTIFACT_RAIL_MAX_WIDTH, (workspace?.clientWidth || window.innerWidth) - (sidebarCollapsed ? sidebarRailWidth : sidebarWidth) - minimumThreadWidth))
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
    } catch (_) {
      if (destroyed || epoch !== speechInstallEpoch) return
      speechInstallError = 'The speech model install state could not be checked. Try again.'
      speechInstallStatus = { state: 'failed' }
      if (pendingUntilTerminal) speechInstallPending = false
    }
  }

  async function openSpeechInstall() {
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
    signedIn: () => auth.name === 'signed-in',
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
    messages
    followNewContent()
  })

  $effect(() => {
    const inWorkspace = auth.name === 'signed-in' && onboarding.name === 'complete'
    const hasConversation = currentThreadId !== null || messages.some(({ role }) => role === 'user')
    void windowTitle.set(inWorkspace && hasConversation ? currentThreadTitle : undefined)
  })

  $effect(() => {
    if (auth.name !== 'signed-in' || onboarding.name !== 'complete') artifactRailOpen = false
  })

  // A signed-out window drives no run, so the desktop drops the active one.
  // The next sign-in restores history and rejoins whatever the runtime runs.
  $effect(() => {
    if (auth.name !== 'signed-in') active = null
  })

  $effect(() => {
    if (auth.name !== 'signed-in') entitlementToast.clear()
  })

  $effect(() => {
    const inWorkspace = auth.name === 'signed-in' && onboarding.name === 'complete' && desktopClientStatus
      && !backgroundServiceNoticeVisible
    if (inWorkspace && !wasInWorkspace && active?.phase !== 'resuming' && composer) {
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
    } catch (err) {
      if (version === authRequestVersion) auth = errorState(action, err)
    }
  }

  function signIn() {
    if (auth.name !== 'signed-out') return
    void run('sign-in')
  }

  onMount(() => {
    let pairingUnlisten
    let registrationRetryUnlisten
    let desktopClientUnlisten
    const readDesktopClientStatus = () => {
      const version = desktopClientStatusVersion
      return tauri?.invoke('attach_listener_status').then((status) => {
        if (version === desktopClientStatusVersion) {
          const connectionRecovered = desktopClientStatus?.connected !== true && status?.connected === true
          applyDesktopClientStatus(status)
          if (connectionRecovered) void run('status')
        }
      }).catch(() => {
        if (version === desktopClientStatusVersion) {
          applyDesktopClientStatus({ connected: false, supervisor_running: false })
        }
        console.error('Desktop client status failed.')
      })
    }
    const startDesktopClientStatus = async () => {
      try {
        const stop = await window.__TAURI__?.event?.listen('desktop-client-status-changed', ({ payload }) => {
          const connectionRecovered = desktopClientStatus?.connected !== true && payload?.connected === true
          desktopClientStatusVersion += 1
          applyDesktopClientStatus(payload)
          if (connectionRecovered) void run('status')
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
      run('status')
      chatController.start()
      entitlementToast.start()
      voiceShortcutManager.start()
    }
    const shortcuts = (event) => {
      const rowPosition = threadRowShortcutPosition(event)
      if (auth.name === 'signed-in' && onboarding.name === 'complete' && rowPosition !== null) {
        event.preventDefault()
        if (sidebarCollapsed || active || threadSwitching) return
        const threadId = freshThread ? threadSummaries[rowPosition - 2]?.threadId : threadSummaries[rowPosition - 1]?.threadId
        if (!threadId || threadId === currentThreadId) return
        void chatController.openThread(threadId)
        return
      }
      if (auth.name === 'signed-in' && onboarding.name === 'complete' && isNewThreadShortcut(event)) {
        event.preventDefault()
        void chatController.newThread()
        return
      }
      if (auth.name === 'signed-in' && onboarding.name === 'complete' && isArtifactRailShortcut(event)) {
        event.preventDefault()
        toggleArtifactRail()
        return
      }
      if (auth.name === 'signed-in' && onboarding.name === 'complete' && isSidebarShortcut(event)) {
        event.preventDefault()
        toggleSidebar()
        return
      }
      if (event.key === 'Escape' && artifactRailOpen) {
        event.preventDefault()
        artifactRailOpen = false
        artifactRailPointer = undefined
      }
      if (event.key === 'Escape' && (dictationRequested || isDictationActive(dictation) || dictationFinishing)) {
        event.preventDefault()
        stopDictation(true)
        return
      }
    }
    document.addEventListener('keydown', shortcuts)
    window.addEventListener('resize', fitArtifactRail)
    let stopDragDrop
    if (tauri) getCurrentWebview().onDragDropEvent(({ payload }) => {
        if (auth.name !== 'signed-in' || active) {
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
      voiceGesture.cleanup()
      transcriptController.cleanup()
      stopDragDrop?.()
      voiceShortcutManager.cleanup()
      document.removeEventListener('keydown', shortcuts)
      window.removeEventListener('resize', fitArtifactRail)
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

  function send() {
    if (sendDisabled()) return
    active ? chatController.queue('steer') : chatController.send()
  }
</script>

<main class:onboarding-active={tauri && onboarding.name !== 'complete'}>
  {#if auth.name !== 'signed-in' || onboarding.name !== 'complete'}
    <div class="lockup">
      <svg width="34" height="34" viewBox="0 0 48 48" aria-hidden="true">
        <path d={markD} stroke-width="4.5" />
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
    <Onboarding {tauri} bind:onboarding />
    {#if onboarding.name === 'complete'}
      {#if auth.name === 'signed-out' || auth.name === 'signing-in'}
      <section class="auth-state">
        <p class="support" aria-live="polite">{auth.name === 'signing-in' ? auth.message : 'Sign in to continue to your workspace.'}</p>
        <button class="primary" class:inactive={auth.name === 'signing-in'} aria-disabled={auth.name === 'signing-in' ? 'true' : undefined} onclick={signIn}>Sign in</button>
      </section>
    {:else if (auth.name === 'signed-in' || (auth.name === 'error' && auth.retry === 'status'))
      && backgroundServiceNoticeVisible}
      <section class="auth-state" aria-live="polite">
        <p class="record error-record">Muniment cannot reach its background service.</p>
        <p class="support">Muniment reconnects on its own.</p>
      </section>
    {:else if auth.name === 'signed-in' && desktopClientStatus}
      <section class="workspace" class:sidebar-collapsed={sidebarCollapsed} class:artifact-open={artifactRailOpen} class:artifact-resizing={artifactRailPointer !== undefined} style:--artifact-rail-width={`${artifactRailWidth}px`} bind:this={workspace}>
        {#if draggingFiles}<div class="drop-affordance" role="status"><strong>Drop files to add them</strong><span>Saved locally · supported images sent with first prompt</span></div>{/if}
        <header class="titlebar">{#if editingThreadTitle}<input class="thread-title" aria-label="Thread name" maxlength="160" bind:this={threadTitleInput} value={threadTitleDraft} oninput={limitThreadTitle} onkeydown={threadTitleKeydown} onblur={commitThreadTitle}>{:else}<h1 class="thread-title-heading" aria-label={currentThreadTitle}><button type="button" class="thread-title" aria-label="Rename thread" title={currentThreadTitle} disabled={!currentThreadId} bind:this={threadTitleButton} onclick={(event) => editThreadTitle(event.currentTarget.title)} onkeydown={threadTitleButtonKeydown}>{currentThreadTitle}</button></h1>{/if}<span class="title-spacer"></span><button type="button" class="quiet" aria-controls="artifact-rail" aria-expanded={artifactRailOpen} aria-keyshortcuts={artifactShortcut} aria-label={`${artifactRailOpen ? 'Close' : 'Open'} artifact rail`} onclick={toggleArtifactRail}>Artifacts <kbd>{shortcutDisplayLabel(artifactShortcut)}</kbd></button></header>
        <aside id="sidebar" class="sidebar">
          <div class="side-brand">
            {#if !sidebarCollapsed}
              <svg width="24" height="24" viewBox="0 0 48 48" aria-hidden="true"><path d={markD} stroke-width="5" /></svg>
              <strong>muniment</strong>
            {/if}
            <!-- One persistent element across both states so activating it never drops keyboard focus. -->
            <button type="button" class="quiet side-toggle" aria-controls="sidebar" aria-expanded={!sidebarCollapsed} aria-keyshortcuts={sidebarKeyShortcut} aria-label={`${sidebarCollapsed ? 'Expand' : 'Collapse'} sidebar`} title={`${sidebarCollapsed ? 'Expand' : 'Collapse'} sidebar (${sidebarHint})`} onclick={toggleSidebar}>
              <svg class="side-icon" width={sidebarCollapsed ? 18 : 16} height={sidebarCollapsed ? 18 : 16} viewBox="0 0 24 24" aria-hidden="true"><rect x="3.5" y="4" width="17" height="16" rx="2.5" /><path d="M9.5 4v16" /><path d={sidebarCollapsed ? 'm14 9 3 3-3 3' : 'm15.5 15-3-3 3-3'} /></svg>
            </button>
          </div>
          <button type="button" class="side-action new-thread" aria-label="New thread" title={sidebarCollapsed ? 'New thread' : null} aria-keyshortcuts={newThreadKeyShortcut} disabled={!!active || threadSwitching} onclick={() => chatController.newThread()}>
            <svg class="side-icon" width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
            {#if !sidebarCollapsed}<span>New thread</span><kbd>{shortcutDisplayLabel(newThreadKeyShortcut)}</kbd>{/if}
          </button>
          {#if !sidebarCollapsed}
            <h2 id="thread-list-title" class="side-label">Threads</h2>
            <ul class="thread-list" aria-labelledby="thread-list-title">
              {#if freshThread}
                <li class="thread-row active-thread" data-fresh-thread aria-current="true" aria-keyshortcuts={threadRowShortcut(1)} title={currentThreadTitle}><span></span><div class="thread-row-title">{currentThreadTitle}</div></li>
              {/if}
              {#each threadSummaries as summary, index (summary.threadId)}
                {@const title = summary.title || 'New thread'}
                {@const current = !freshThread && summary.threadId === (currentThreadId ?? threadSummaries[0]?.threadId)}
                {@const rowTitle = current ? summary.title || currentThreadTitle : title}
                {@const rowPosition = index + 1 + (freshThread ? 1 : 0)}
                <li class="thread-record">
                  {#if current}
                    <div class="thread-row active-thread" aria-current="true" aria-keyshortcuts={rowPosition <= 9 ? threadRowShortcut(rowPosition) : undefined} title={rowTitle}><span></span><div class="thread-row-title">{rowTitle}</div><time datetime={summary.updatedAt} title={fullDateTime(summary.updatedAt)}>{relativeTime(summary.updatedAt)}</time></div>
                  {:else}
                    <button class="thread-row" data-thread-id={summary.threadId} title={rowTitle} aria-keyshortcuts={rowPosition <= 9 ? threadRowShortcut(rowPosition) : undefined} aria-disabled={active ? 'true' : undefined} onclick={() => chatController.openThread(summary.threadId)}><span></span><div class="thread-row-title">{rowTitle}</div><time datetime={summary.updatedAt} title={fullDateTime(summary.updatedAt)}>{relativeTime(summary.updatedAt)}</time></button>
                  {/if}
                  {#if deletingThreadId === summary.threadId}
                    <div class="thread-delete-confirm" role="group" aria-label={`Delete ${rowTitle}?`}>
                      <span>Delete “{rowTitle}”?</span>
                      <button type="button" disabled={deletePending} onclick={() => confirmDeleteThread(summary.threadId)} onkeydown={deleteConfirmKeydown}>Delete</button>
                      <button type="button" disabled={deletePending} onclick={cancelDeleteThread} onkeydown={deleteConfirmKeydown}>Cancel</button>
                    </div>
                  {:else}
                    <button type="button" class="thread-delete" data-delete-thread={summary.threadId} aria-label={`Delete ${rowTitle}`} disabled={!!active || threadSwitching} onclick={() => askToDeleteThread(summary.threadId)}>Delete</button>
                  {/if}
                </li>
              {/each}
            </ul>
            {#if moreThreads}
              <button type="button" class="older-threads" disabled={loadingOlderThreads} onclick={loadOlderThreads}>Older threads</button>
            {/if}
          {/if}
          <button class="side-action home-settings" aria-label={sidebarCollapsed ? 'Home settings' : null} title={sidebarCollapsed ? 'Home settings' : null} onclick={() => { onboarding = onboardingSettingsState(onboarding) }}><svg class="side-icon" width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 10.5 12 4.75l7.5 5.75V19a1.5 1.5 0 0 1-1.5 1.5H6A1.5 1.5 0 0 1 4.5 19z" /><path d="M9.75 20.5v-5.75h4.5v5.75" /></svg>{#if !sidebarCollapsed}<span>Home settings</span>{/if}</button>
          {#if !sidebarCollapsed}
            <AccessPanel {tauri} subject={auth.subject} onSignOut={() => run('sign-out')} escapeBlocked={() => dictationRequested || isDictationActive(dictation)} voiceShortcut={globalVoiceShortcutValue} voiceShortcutChanging={globalVoiceChanging} onVoiceShortcutChange={changeVoiceShortcut} defaultVoiceShortcut={holdToTalkShortcut()} />
          {/if}
        </aside>
        <div class="thread-shell">
        <div class="thread" role="region" aria-label={`Transcript: ${currentThreadTitle}`} bind:this={thread} onscroll={handleThreadScroll}>
          {#if historyError}<p class="history-error" role="alert">{historyError} {#if historyErrorAction}<button onclick={historyErrorAction.run}>{historyErrorAction.label}</button>{/if}</p>{/if}
          {#if messages.length === 0}<p class="empty">Ask anything. Your org's routing decides which model answers.</p>{/if}
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
            {@const activity = message.run.toolActivity ?? []}
            {@const groupedIds = parallelTools.get(message.run.id) ?? []}
            {@const groupedTools = activity.filter((tool) => groupedIds.includes(tool.effectId))}
            {@const singleTools = activity.filter((tool) => !groupedIds.includes(tool.effectId))}
            <div class="response">
              {#if message.run.phase === 'thinking'}
                <span class="thinking" out:thinkingSettle><svg width="17" height="17" viewBox="0 0 48 48" aria-label="Thinking"><path d={thinkingMarkD} fill-rule="evenodd" /></svg><span>Routing</span></span>
              {:else if message.run.phase === 'streaming'}<p class="response-prose streaming" use:streamingUnderline={message.run.text}>{message.run.text}<span class="caret" aria-hidden="true"></span><span class="streaming-rule" aria-hidden="true"></span></p>
              {:else if ['complete', 'failed', 'interrupted', 'cancelled'].includes(message.run.phase)}<AssistantMarkdown text={message.run.text} />
              {:else}<p class="response-prose">{message.run.text}</p>{/if}
              {#each message.run.appliedDiffs ?? [] as appliedDiff}
                <div class="applied-diff tool-card">
                  <strong>Applied file changes</strong>
                  {#if appliedDiff.diff}<CodeDiff codeDiff={appliedDiff.diff} />
                  {:else}<p>The changes were applied, but their record is no longer stored.</p>{/if}
                </div>
              {/each}
              {#if message.run.phase === 'failed'}<div class="run-error">Reply failed. <button disabled={dictationBusy() || runtimeUpgradePending()} onclick={() => { draft = message.run.prompt; chatController.send() }}>Try again</button></div>{/if}
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
              {#if groupedTools.length}
                <div class="tool-card tool-group" role="group" aria-label={`Parallel tool activity: ${groupedTools.map((tool) => `${toolName(tool)} ${toolStatus(tool)}`).join(', ')}`}>
                  <div class="tool-group-title">Parallel tool activity</div>
                  <ul class="tool-list">
                    {#each groupedTools as tool}
                      <li class:tool-running={toolStatus(tool) === 'running'} class:tool-failed={toolStatus(tool) === 'failed'} class="tool-row" aria-label={`${toolName(tool)} ${toolStatus(tool)}`}>
                        <span class="tool-dot" aria-hidden="true"></span><span class="tool-name">{toolName(tool)}</span><span class="tool-status">{toolStatus(tool)}</span>
                      </li>
                    {/each}
                  </ul>
                </div>
              {/if}
              {#if singleTools.length}
                <ul class="tool-list" aria-label="Tool activity">
                  {#each singleTools as tool}
                    <li class:tool-running={toolStatus(tool) === 'running'} class:tool-failed={toolStatus(tool) === 'failed'} class="tool-card tool-row" aria-label={`${toolName(tool)} ${toolStatus(tool)}`}>
                      <span class="tool-dot" aria-hidden="true"></span><span class="tool-name">{toolName(tool)}</span><span class="tool-status">{toolStatus(tool)}</span>
                    </li>
                  {/each}
                </ul>
              {/if}
              {#if message.run.phase === 'complete'}
                {@const summary = receiptSummary(message.run.receipt)}
                {#if summary.route !== null || summary.detail}
                  {@const expanded = expandedReceipts.has(message.run.id)}
                  {@const rows = receiptRows(message.run.receipt, message.run.recalls)}
                  <button class="provenance" aria-expanded={expanded} aria-label={`${expanded ? 'Collapse' : 'Expand'} receipt: ${receiptLabel(message.run.receipt)}`} onclick={() => toggleReceipt(message.run.id)}><span class:expanded class="receipt-marker" aria-hidden="true"></span>{#if summary.route !== null}<span class="route-segment">{summary.route}</span>{/if}{summary.separator}{summary.detail}</button>
                  {#if expanded}
                    <dl class="receipt-record">
                      {#each rows as row}
                        <div><dt>{row.label}</dt><dd class:route-value={row.route}>{row.value}{#each row.files ?? [] as file}<span class="recall-file">{file}</span>{/each}</dd></div>
                      {/each}
                    </dl>
                  {/if}
                {:else}
                  <p class="provenance">Receipt unavailable</p>
                {/if}
                {@const failure = copyFailure(copy, message.run.id, modifierLabel)}
                <!-- §3.2's action row, copy only in this slice. -->
                <div class="message-actions">
                  <button type="button" onclick={() => copyResponse(message.run)}>{#if copyConfirmed(copy, message.run.id)}<svg class="action-icon" width="14" height="14" viewBox="0 0 24 24" aria-hidden="true"><path d="M20 6 9 17l-5-5" /></svg>{:else}<svg class="action-icon" width="14" height="14" viewBox="0 0 24 24" aria-hidden="true"><rect x="8" y="8" width="14" height="14" rx="2" /><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" /></svg>{/if}{copyLabel(copy, message.run.id)}</button>
                </div>
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
        <div class="composer">
          {#if selectedFiles.length}
            <ul class="attachments" aria-label="Selected files">
              {#each selectedFiles as file}
                <li><span>{file.displayName}</span><span>{formatByteSize(file.byteLength)}</span><button type="button" aria-label={`Remove ${file.displayName}`} onclick={() => { selectedFiles = selectedFiles.filter(({ path }) => path !== file.path) }}>Remove</button></li>
              {/each}
            </ul>
          {/if}
          <div class="composer-input">
            <label class="visually-hidden" for="composer-message">Message</label>
            <textarea id="composer-message" aria-describedby="composer-hint" bind:this={composer} use:focusComposerOnMount bind:value={draft} oninput={composerInput} onkeydown={keydown} rows="2" placeholder={active?.phase === 'resuming' ? 'Resuming interrupted reply…' : 'Ask anything'} disabled={composer && (active?.phase === 'resuming' || threadSwitching)}></textarea>
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
            {#if isDictationActive(dictation)}
              <span id="composer-hint" class="capture-status" role="status">
                <span class="capture-meter" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i></span>
                {dictation.state === 'starting' ? 'Starting local dictation…' : 'Listening on this device…'}
              </span>
            {:else}
              <span id="composer-hint">{active?.phase === 'resuming' ? 'Reopening the existing secure session…' : active && active.id !== 'pending' ? '⏎ steers this reply · queue as follow-up' : 'Routing is automatic. Every reply carries its receipt.'}</span>
            {/if}
            <div class="composer-actions">
              <button type="button" class="quiet" aria-pressed={isDictationActive(dictation)} aria-keyshortcuts={ariaKeyShortcut(globalVoiceShortcutValue)} disabled={!!active || dictationFinishing} onpointerdown={voicePointerDown} onpointerup={voicePointerEnd} onpointercancel={voicePointerEnd} onkeydown={voiceKeyDown} onkeyup={voiceKeyUp} onclick={voiceClick}>Voice</button>
              {#if !active}<button type="button" class="quiet" onclick={chooseFiles}>Add files</button>{/if}
              {#if active?.phase === 'resuming'}
                <button disabled>Resuming…</button>
              {:else if active && active.id !== 'pending'}
                <button class="quiet follow-up" disabled={!draft.trim() || runtimeUpgradePending()} onclick={() => chatController.queue('followUp')}>Queue follow-up</button>
                <button onclick={() => chatController.cancel()}>Stop</button>
              {/if}
              <button class="primary" aria-disabled={sendDisabled() ? 'true' : undefined} onclick={send}>Send</button>
            </div>
          </div>
          {#if dictation.state === 'modelNotInstalled'}
            <section class="speech-install-card" aria-labelledby="speech-install-title">
              <strong id="speech-install-title">Speech model install</strong>
              {#if speechInstallFacts}
                <dl>
                  <div><dt>Download</dt><dd>{formatBytes(speechInstallFacts.totalDownloadBytes)}</dd></div>
                  <div><dt>Source</dt><dd>{speechInstallFacts.sourceRepository}</dd></div>
                  <div><dt>Speech model license</dt><dd>{speechInstallFacts.speechModelLicense}</dd></div>
                  <div><dt>Voice activity model license</dt><dd>{speechInstallFacts.voiceActivityModelLicense}</dd></div>
                  <div><dt>Free disk required</dt><dd>{formatBytes(speechInstallFacts.requiredFreeBytes)}</dd></div>
                </dl>
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
          {:else if dictationError}<div class="dictation-error" role="alert">{dictationError}</div>{/if}
          {#if globalVoiceError}<div class="dictation-error" role="alert">The system-wide voice shortcut is unavailable. Voice remains available from the button.</div>{/if}
        </div>
        {#if entitlementToastVisible}
          <div class="entitlement-toast" role="status">Your access changed. Some models or connections may differ.</div>
        {/if}
        {#if artifactRailOpen}
          <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
          <div
            class="artifact-divider"
            class:dragging={artifactRailPointer !== undefined}
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
              <strong>No artifacts yet</strong>
              <p>Artifacts created in this thread will appear here.</p>
            </div>
          </aside>
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
    align-content: center;
    box-sizing: border-box;
    padding: 24px;
  }

  /* Lockup (§1.8): static ink mark at rest beside the wordmark,
     Schibsted 600, lowercase, −1% tracking. */
  .lockup {
    display: flex;
    align-items: center;
    gap: 13px;
  }

  .lockup path {
    fill: none;
    stroke: var(--ink);
    stroke-linecap: round;
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

  button:hover:not(:disabled):not([aria-disabled="true"]) {
    border-color: var(--muted);
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

  .workspace { position: fixed; inset: 0; display: grid; grid-template-rows: 52px 1fr auto; }
  .workspace { grid-template-columns: 260px minmax(0, 1fr); grid-template-areas: "title title" "side thread" "side composer"; transition: grid-template-columns 180ms ease; }
  .workspace.artifact-resizing { transition: none; }
  .workspace.artifact-open { grid-template-columns: 260px minmax(320px, 1fr) var(--artifact-rail-width); grid-template-areas: "title title title" "side thread rail" "side composer rail"; }
  /* §2.1: the same 180ms grid transition carries the sidebar down to a 52px icon rail. */
  .workspace.sidebar-collapsed { grid-template-columns: 52px minmax(0, 1fr); }
  .workspace.sidebar-collapsed.artifact-open { grid-template-columns: 52px minmax(320px, 1fr) var(--artifact-rail-width); }
  .workspace.sidebar-collapsed .titlebar { padding-left: 70px; }
  .workspace.sidebar-collapsed .drop-affordance { left: 52px; }
  .entitlement-toast { position: fixed; z-index: 4; left: 50%; bottom: 24px; max-width: calc(100% - 48px); padding: 10px 14px; transform: translateX(-50%); border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); box-shadow: var(--shadow-overlay); animation: toast-enter var(--motion-popover) var(--ease-out); }
  .drop-affordance { position: fixed; z-index: 4; inset: 52px 0 0 260px; display: grid; place-content: center; gap: 5px; background: color-mix(in srgb, var(--paper) 92%, transparent); border: 1px dashed var(--muted); color: var(--ink); text-align: center; pointer-events: none; }
  .drop-affordance span { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .titlebar { grid-area: title; display: flex; align-items: center; padding: 0 18px 0 278px; border-bottom: 1px solid var(--border); background: var(--surface); transition: padding-left 180ms ease; }
  .thread-title-heading { min-width: 0; max-width: 100%; font: inherit; }
  .thread-title { min-width: 0; max-width: 100%; overflow: hidden; padding: 2px; border: 0; background: transparent; color: var(--ink); font: inherit; font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  button.thread-title:disabled { opacity: 1; }
  kbd { margin-left: 10px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .title-spacer { flex: 1; }
  .sidebar { grid-area: side; min-width: 0; display: flex; flex-direction: column; padding: 14px 10px 10px; background: var(--surface); border-right: 1px solid var(--border); }
  .side-brand { display: flex; align-items: center; gap: 10px; padding: 2px 8px 16px; }
  .side-brand path { fill: none; stroke: var(--ink); stroke-linecap: round; }
  .side-toggle { display: flex; align-items: center; justify-content: center; margin-left: auto; padding: 4px; line-height: 0; }
  .side-toggle:hover:not(:disabled), .side-toggle:focus-visible { border-color: transparent; background: var(--faint); }
  .side-toggle:hover:not(:disabled) .side-icon, .side-toggle:focus-visible .side-icon { color: var(--ink); }
  /* §1.7: one geometric 1.6px-stroke icon set, sized to the mockup's rail. */
  .side-icon { flex: none; display: block; color: var(--muted); }
  .side-icon rect, .side-icon path { fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .side-action, .thread-row { width: 100%; display: flex; align-items: center; gap: 9px; padding: 7px 8px; border-color: transparent; background: transparent; text-align: left; }
  .thread-list { min-height: 0; padding: 0; overflow-y: auto; list-style: none; }
  .older-threads { width: 100%; margin-top: 4px; border-color: transparent; background: transparent; color: var(--muted); }
  .thread-record { position: relative; }
  .thread-row { font: inherit; font-size: var(--text-13); color: var(--ink); border: 1px solid transparent; border-radius: var(--radius-control); }
  button.thread-row:hover:not([aria-disabled="true"]) { background: var(--faint); }
  button.thread-row[aria-disabled="true"] { opacity: .55; }
  .thread-row time { margin-left: auto; color: var(--muted); font: var(--text-provenance) var(--font-mono); }
  .thread-row > span { flex: 0 0 5px; }
  .thread-row-title { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .thread-delete { position: absolute; top: 4px; right: 5px; min-width: 24px; min-height: 24px; padding: 3px 6px; border-color: transparent; background: transparent; color: var(--muted); font: var(--text-12) var(--font-mono); opacity: 0; transition: opacity 120ms ease; }
  .thread-record:hover .thread-delete, .thread-record:focus-within .thread-delete { opacity: 1; }
  .thread-delete:hover:not(:disabled) { border-color: transparent; background: var(--faint); color: var(--ink); }
  .thread-delete:focus-visible, .thread-delete-confirm button:focus-visible { outline-color: var(--ink); }
  .thread-delete-confirm { position: absolute; inset: 0; display: flex; align-items: center; justify-content: flex-end; gap: 5px; min-width: 0; padding: 5px 7px; border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); }
  .thread-delete-confirm > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .thread-delete-confirm button { flex: none; min-width: 24px; min-height: 24px; padding: 3px 6px; border-color: transparent; background: transparent; color: var(--ink); font: inherit; }
  .thread-delete-confirm button:hover:not(:disabled) { background: var(--faint); }
  .side-action span { flex: 1; }
  .new-thread kbd { margin-left: auto; }
  /* Collapsed rail: icon-only controls, names carried by aria-label + tooltip. */
  .workspace.sidebar-collapsed .sidebar { padding: 14px 6px 10px; }
  .workspace.sidebar-collapsed .side-brand { padding: 0 0 14px; }
  .workspace.sidebar-collapsed .side-toggle { width: 100%; margin: 0; padding: 9px 0; }
  .workspace.sidebar-collapsed .side-action { justify-content: center; gap: 0; padding: 9px 0; }
  /* The pinned control is its own group once the threads list is gone. */
  .workspace.sidebar-collapsed .home-settings { position: relative; margin-top: auto; }
  .workspace.sidebar-collapsed .home-settings::before { content: ''; position: absolute; inset: -9px -6px auto; height: 1px; background: var(--border); }
  .side-label { margin: 20px 8px 5px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .active-thread { background: var(--faint); }
  /* §1.2 forbids signal on selection states; the mockup's current-thread dot is ink. */
  .active-thread > span { width: 5px; height: 5px; border-radius: 50%; background: var(--ink); }
  .quiet { background: transparent; border-color: transparent; }
  .artifact-divider { grid-area: rail; z-index: 2; align-self: stretch; width: 9px; margin-left: -4px; padding: 0; border: 0; border-radius: 0; background: transparent; cursor: col-resize; touch-action: none; }
  .artifact-divider::after { content: ''; display: block; width: 1px; height: 100%; margin-left: 4px; background: var(--border); }
  .artifact-divider:hover::after, .artifact-divider:focus-visible::after, .artifact-divider.dragging::after { width: 2px; margin-left: 3px; background: var(--muted); }
  .artifact-divider:focus-visible { outline: 2px solid var(--ink); outline-offset: -2px; }
  .artifact-rail { grid-area: rail; min-width: 0; padding: 22px 24px; overflow-y: auto; background: var(--surface); }
  .artifact-rail header { padding-bottom: 15px; border-bottom: 1px solid var(--border); }
  .artifact-rail h2 { margin: 3px 0 0; font-size: var(--text-17); }
  .artifact-empty { display: grid; place-items: center; align-content: center; min-height: 45%; text-align: center; }
  .artifact-empty strong { font-weight: 600; }
  .artifact-empty p { max-width: 250px; margin: 7px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); line-height: 1.5; }
  .thread-shell { grid-area: thread; position: relative; min-height: 0; }
  .thread { width: min(760px, calc(100% - 48px)); height: 100%; margin: 0 auto; padding: 42px 0; overflow-y: auto; }
  .latest { position: absolute; left: 50%; bottom: 14px; transform: translateX(-50%); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); font: var(--text-12) var(--font-mono); box-shadow: var(--shadow-overlay); }
  .empty { color: var(--muted); text-align: center; margin-top: 18vh; }
  .user-turn { margin: 0 0 28px auto; }
  .user-message { width: fit-content; max-width: 78%; margin-left: auto; padding: 9px 13px; overflow-wrap: anywhere; background: var(--faint); border-radius: var(--radius-panel); }
  .user-message > p { margin: 0; white-space: pre-wrap; }
  .missing-prompt { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .message-attachments { display: grid; justify-items: end; gap: 4px; margin: 8px 0 0; padding: 0; list-style: none; }
  .message-attachments li { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 5px 8px; max-width: 100%; padding: 5px 8px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .message-attachments strong { flex-basis: 100%; color: var(--muted); font-weight: 400; font-size: var(--text-12); }
  .attachment-delivery-rule { margin: 4px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .response { margin: 0 0 34px; }
  .response-prose { max-width: 92%; white-space: pre-wrap; overflow-wrap: anywhere; }
  .streaming { position: relative; }
  .streaming-rule { position: absolute; height: 2px; background: var(--signal); pointer-events: none; }
  .caret { display: inline-block; height: 1em; border-right: 2px solid var(--signal); margin-left: 2px; vertical-align: -2px; animation: blink 800ms step-end infinite; }
  .thinking { display: flex; align-items: center; gap: 9px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .thinking path { fill: var(--signal); animation: breathe 1.8s ease-in-out infinite; }
  .tool-card { margin-top: 8px; padding: 8px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); font: var(--text-13) var(--font-mono); }
  .tool-list { margin: 0; padding: 0; list-style: none; }
  .tool-row { display: flex; align-items: center; gap: 8px; min-height: 20px; }
  .tool-group-title { margin-bottom: 4px; color: var(--muted); }
  .tool-group .tool-row + .tool-row { margin-top: 4px; }
  .tool-dot { width: 7px; height: 7px; flex: 0 0 auto; border-radius: 50%; background: currentColor; }
  .tool-name { min-width: 0; overflow-wrap: anywhere; }
  .tool-status { margin-left: auto; }
  .tool-running { color: var(--signal); }
  .tool-running .tool-dot { animation: tool-pulse 1.4s ease-in-out infinite; }
  .tool-failed .tool-status { color: var(--oxide); }
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
  .provenance { display: flex; align-items: center; min-width: 24px; min-height: 24px; margin-top: 10px; padding: 0; border: 0; background: transparent; color: var(--muted); font: var(--text-provenance)/1.45 var(--font-mono); font-variant-numeric: tabular-nums; text-align: left; overflow-wrap: anywhere; }
  /* §2.2 mono 11.5px; §1.4 records line up their figures. The shorthand resets
     font-variant-numeric, so tabular-nums follows it. */
  .provenance:hover:not(:disabled) { color: var(--ink); }
  .receipt-marker { display: inline-block; width: 5px; height: 5px; margin-right: 7px; border-right: 1px solid currentColor; border-bottom: 1px solid currentColor; transform: rotate(-45deg); transition: transform 120ms ease; vertical-align: 1px; }
  .receipt-marker.expanded { transform: rotate(45deg); }
  /* §1.2 permits --signal on the route segment only. */
  .provenance .route-segment { color: var(--signal); }
  .receipt-record { box-sizing: border-box; width: 329px; max-width: 100%; margin: 8px 0 0; padding: 8px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .receipt-record div { display: grid; grid-template-columns: 88px minmax(0, 1fr); gap: 12px; }
  .receipt-record dd { margin: 0; font-family: var(--font-mono); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .receipt-record .recall-file { display: block; }
  .receipt-record .route-value { color: var(--signal); }
  /* §3.2: hover or focus reveals the row. Only opacity carries the reveal. The row
     always holds its space, so nothing reflows and nothing is ever obscured, and the
     button keeps its place in the tab order. `visibility: hidden` would strip it from
     that order exactly as `display: none` does, which would make focus unreachable
     and the :focus-within reveal below unreachable with it. */
  .message-actions { display: flex; gap: 2px; margin-top: 8px; opacity: 0; transition: opacity 120ms ease; }
  .response:hover .message-actions, .response:focus-within .message-actions { opacity: 1; }
  .message-actions button { display: inline-flex; align-items: center; gap: 5px; padding: 4px 8px; border-color: transparent; background: transparent; color: var(--muted); font-size: var(--text-12); }
  .message-actions button:hover:not(:disabled) { border-color: transparent; background: var(--faint); color: var(--ink); }
  /* §1.2: focus rings are ink, never signal. */
  .message-actions button:focus-visible { outline-color: var(--ink); }
  .message-actions button:disabled { opacity: .45; }
  /* Same §1.7 icon geometry as the rail, tracking whatever ink its button carries. */
  .action-icon { flex: none; display: block; color: inherit; }
  .action-icon rect, .action-icon path { fill: none; stroke: currentColor; stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .copy-failure { margin-top: 4px; }
  .run-error { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .cancel-error, .history-error { margin: 0 0 8px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  /* The notice keeps the background service register at the composer 12px scale.
     A 15px support line would compete with the draft text. */
  .update-notice { display: grid; gap: 2px; margin: 6px 0 8px; }
  .update-notice .support { font-size: var(--text-12); }
  .run-error button { min-width: 24px; min-height: 24px; padding: 2px 6px; background: transparent; font: inherit; }
  .composer { grid-area: composer; width: min(760px, calc(100% - 48px)); margin: 0 auto 24px; padding: 12px; background: var(--surface); border: 1px solid var(--border); border-radius: var(--radius-panel); }
  .composer:focus-within { border-color: var(--muted); }
  .attachments { display: flex; flex-wrap: wrap; gap: 6px; margin: 0 0 8px; padding: 0; list-style: none; }
  .attachments li { display: flex; align-items: center; gap: 6px; max-width: 100%; padding: 4px 6px 4px 9px; border: 1px solid var(--border); border-radius: var(--radius-chip); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .attachments span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .attachments button { padding: 1px 5px; border: 0; background: transparent; color: inherit; font-size: var(--text-12); }
  .composer-input { position: relative; }
  /* No padding and no border: the composer supplies both, so the measured
     scrollHeight is pure text and the overlay lands on the same grid. */
  textarea { display: block; width: 100%; resize: none; padding: 0; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; }
  /* The input no longer keeps a spare empty row once it grows, so the action
     row carries the gap itself, matching the owner mockup's 8px .comprow rhythm. */
  .composer-row { display: flex; justify-content: space-between; align-items: center; margin-top: 8px; color: var(--muted); font-size: var(--text-12); }
  .composer-actions { display: flex; align-items: center; gap: 6px; }
  .capture-status { display: flex; align-items: center; gap: 8px; font-family: var(--font-mono); }
  .capture-meter { height: 14px; display: flex; align-items: center; gap: 2px; }
  .capture-meter i { width: 2px; height: 6px; background: var(--muted); animation: capture 900ms ease-in-out infinite alternate; }
  .capture-meter i:nth-child(2), .capture-meter i:nth-child(4) { height: 10px; animation-delay: -300ms; }
  .capture-meter i:nth-child(3) { height: 14px; animation-delay: -600ms; }
  .dictation-error { margin-top: 7px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .speech-install-card { margin-top: 9px; padding: 10px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--ink); font: var(--text-12) var(--font-mono); }
  .speech-install-card strong { font-weight: 600; }
  .speech-install-card dl { margin: 7px 0; }
  .speech-install-card dl div { display: grid; grid-template-columns: minmax(130px, 1fr) minmax(0, 2fr); gap: 12px; }
  .speech-install-card dt { color: var(--muted); }
  .speech-install-card dd { margin: 0; overflow-wrap: anywhere; }
  .speech-install-card p { margin: 7px 0 0; color: var(--muted); }
  .speech-install-progress { display: grid; gap: 4px; margin-top: 9px; color: var(--muted); }
  .speech-install-progress progress { width: 100%; height: 6px; accent-color: var(--muted); }
  .speech-install-card button { margin-top: 7px; padding: 4px 8px; font: inherit; }
  .speech-install-card .speech-install-error { color: var(--oxide); }
  .follow-up { color: var(--muted); font-family: var(--font-mono); }
  @media (max-width: 1100px) {
    .workspace.artifact-open .composer-row { flex-wrap: wrap; gap: 8px; }
    .workspace.artifact-open .composer-row > span { flex-basis: 100%; }
    .workspace.artifact-open .composer-actions { width: 100%; flex-wrap: wrap; justify-content: flex-end; }
  }
  @keyframes blink { 50% { opacity: 0; } }
  @keyframes breathe { 50% { opacity: .45; } }
  @keyframes tool-pulse { 50% { opacity: .3; transform: scale(.75); } }
  @keyframes capture { to { transform: scaleY(.55); } }
  @keyframes toast-enter { from { opacity: 0; transform: translate(-50%, 2px); } }
  @media (prefers-reduced-motion: reduce) {
    /* Unlike the blanket duration rule, removing this animation keeps the meter
       at its full-height resting state instead of the keyframe's 55% endpoint. */
    .capture-meter i { animation: none; }
    .thinking path { animation: none; }
    .receipt-marker { transition: none; }
  }
</style>
