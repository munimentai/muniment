<script>
  import { onMount, tick, untrack } from 'svelte'
  import { cubicOut } from 'svelte/easing'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { confirm, open } from '@tauri-apps/plugin-dialog'
  import { register, unregister } from '@tauri-apps/plugin-global-shortcut'

  import AccessPanel from './lib/AccessPanel.svelte'
  import Onboarding from './lib/Onboarding.svelte'
  import { ARTIFACT_RAIL_MAX_WIDTH, ARTIFACT_RAIL_MIN_WIDTH, artifactRailShortcut, artifactRailWidthFromKey, artifactRailWidthFromPointer, clampArtifactRailWidth, defaultArtifactRailWidth, isArtifactRailShortcut, shortcutDisplayLabel } from './lib/artifact-rail-state.js'
  import { bootState, errorState, statusState, waitingState } from './lib/auth-state.js'
  import { ringPath, solidMilledRingPath } from './lib/mark.js'
  import { applyBufferedChatEvents, applyChatEvent, composerAction, formatByteSize, historyMessages, receiptLabel, receiptRows, receiptSummary, runAnnouncement, toolName, toolStatus } from './lib/chat-state.js'
  import { composerHeight } from './lib/composer-size.js'
  import { createDictationController } from './lib/dictation-controller.js'
  import { appendTranscript, ariaKeyShortcut, dictationTransforms, handsFreeActivationDelay, holdToTalkShortcut, isDictationActive } from './lib/dictation-state.js'
  import { COPY_CONFIRMATION_MS, copyAnnouncement, copyConfirmed, copyFailure, copyLabel, copyResult } from './lib/message-actions.js'
  import { onboardingLoadingState, onboardingSettingsState } from './lib/onboarding-state.js'
  import { scrollFollowState } from './lib/scroll-follow.js'
  import { SIDEBAR_STORAGE_KEY, isSidebarShortcut, parseSidebarCollapsed, serializeSidebarCollapsed, sidebarShortcut } from './lib/sidebar-state.js'
  import { streamingUnderlineGeometry } from './lib/streaming-underline.js'
  import { createVoiceShortcutManager } from './lib/voice-shortcut.js'

  const markD = ringPath()
  const thinkingMarkD = solidMilledRingPath()
  const version = __APP_VERSION__

  function thinkingSettle() {
    const reducedMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches ?? false
    return {
      duration: reducedMotion ? 0 : 180,
      easing: cubicOut,
      css: (t) => `opacity: ${t}; transform: translateY(${(1 - t) * -2}px) scale(${0.96 + t * 0.04})`,
    }
  }

  const tauri = window.__TAURI__?.core
  let auth = $state(bootState)
  let draft = $state('')
  let selectedFiles = $state([])
  let submitError = $state('')
  let messages = $state([])
  let active = $state(null)
  // The transcript is not a live region; only the run the user is waiting on is
  // announced, and only when its phase changes. Restored history announces nothing.
  let announcedRun = $state(null)
  let announcement = $derived(runAnnouncement(announcedRun))
  let cancelError = $state('')
  let queueError = $state('')
  let historyError = $state('')
  let buffered = new Map()
  let unlisten
  let thread = $state()
  let pinned = $state(true)
  let hasContentBelow = $state(false)
  let lastScrollTop = 0
  let expandedReceipts = $state(new Set())
  let parallelTools = $state(new Map())
  let submissionSequence = 0
  let draggingFiles = $state(false)
  let dictation = $state({ state: 'idle' })
  let dictationError = $state('')
  let dictationCommandPending = $state(false)
  let dictationRequested = false
  let dictationDraftSnapshot = $state('')
  let dictationTranscript = $state('')
  let dictationFinishing = $state(false)
  let dictationPolishing = $state(false)
  let dictationTransformPending = $state(false)
  let eligibleDictation = $state(null)
  let suppressVoiceClick = false
  let voiceClickTimer
  let voiceReleaseTimer
  let voiceReleasePending = false
  let voiceActivationStartedAt
  let voiceActivationSource
  let pendingVoiceActivationAt
  let pendingVoiceActivationSource
  let handsFreeDictation = false
  let ignoreVoiceRelease = false
  let ignoreGlobalVoiceRelease = false
  let voicePointerId
  let voiceKey
  let globalVoiceHeld = false
  let globalVoiceRegistered = false
  let globalVoiceError = $state(false)
  let globalVoiceShortcutValue = $state(holdToTalkShortcut())
  let globalVoiceChanging = $state(true)
  let composer = $state()
  let polishPreview = $state()
  let composerRow = $state()
  let composerInputDraft
  let wasInWorkspace = false
  let onboarding = $state(onboardingLoadingState)
  let artifactRailOpen = $state(false)
  let artifactRailWidth = $state(defaultArtifactRailWidth(window.innerWidth))
  let artifactRailMaximum = $state(ARTIFACT_RAIL_MAX_WIDTH)
  let artifactRailPointer = $state()
  let workspace = $state()
  const artifactShortcut = artifactRailShortcut()
  let destroyed = false
  const sidebarWidth = 260
  const sidebarRailWidth = 52
  const minimumThreadWidth = 320
  let sidebarCollapsed = $state(storedSidebarCollapsed())
  const sidebarKeyShortcut = sidebarShortcut()
  const modifierLabel = shortcutDisplayLabel(sidebarKeyShortcut).slice(0, -1)
  const sidebarHint = `${modifierLabel}\\`
  // The newest copy attempt in the thread, or null once its confirmation lapses.
  let copy = $state(null)
  let copyEpoch = 0
  let copyTimer

  // A read from a blocked or corrupt store must not keep the shell from
  // rendering; §2.1's documented default is expanded.
  function storedSidebarCollapsed() {
    try {
      return parseSidebarCollapsed(localStorage.getItem(SIDEBAR_STORAGE_KEY))
    } catch (_) {
      return false
    }
  }

  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed
    try { localStorage.setItem(SIDEBAR_STORAGE_KEY, serializeSidebarCollapsed(sidebarCollapsed)) } catch (_) {}
    fitArtifactRail()
  }

  function availableArtifactRailWidth() {
    return Math.max(ARTIFACT_RAIL_MIN_WIDTH, Math.min(ARTIFACT_RAIL_MAX_WIDTH, (workspace?.clientWidth || window.innerWidth) - (sidebarCollapsed ? sidebarRailWidth : sidebarWidth) - minimumThreadWidth))
  }

  function fitArtifactRail() {
    if (!artifactRailOpen) return
    artifactRailMaximum = availableArtifactRailWidth()
    artifactRailWidth = clampArtifactRailWidth(artifactRailWidth, artifactRailMaximum)
  }

  function resetArtifactRailWidth() {
    artifactRailMaximum = availableArtifactRailWidth()
    artifactRailWidth = clampArtifactRailWidth(defaultArtifactRailWidth(workspace?.clientWidth || window.innerWidth), artifactRailMaximum)
  }

  function toggleArtifactRail() {
    artifactRailOpen = !artifactRailOpen
    artifactRailPointer = undefined
    if (artifactRailOpen) resetArtifactRailWidth()
  }

  function artifactRailPointerDown(event) {
    if (event.button !== 0 || artifactRailPointer !== undefined) return
    event.preventDefault()
    event.currentTarget.focus()
    artifactRailPointer = event.pointerId
    event.currentTarget.setPointerCapture?.(event.pointerId)
  }

  function artifactRailPointerMove(event) {
    if (event.pointerId !== artifactRailPointer) return
    artifactRailWidth = artifactRailWidthFromPointer(event.clientX, workspace.getBoundingClientRect().right, artifactRailMaximum)
  }

  function artifactRailPointerEnd(event) {
    if (event.pointerId !== artifactRailPointer) return
    artifactRailPointer = undefined
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
  }

  function artifactRailKeydown(event) {
    const width = artifactRailWidthFromKey(artifactRailWidth, event.key, artifactRailMaximum)
    if (width === artifactRailWidth && !['Home', 'End', 'ArrowLeft', 'ArrowRight'].includes(event.key)) return
    event.preventDefault()
    artifactRailWidth = width
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
      dictationPolishing = state.polishing
      dictationTransformPending = state.transformPending
      eligibleDictation = state.eligible
      dictationDraftSnapshot = state.draftSnapshot
      dictationTranscript = state.transcript
    },
    onError: (message) => { dictationError = message },
    onInactive: () => {
      clearPendingVoiceRelease()
      handsFreeDictation = false
    },
    onCancel: () => {
      voicePointerId = undefined
      voiceKey = undefined
    },
    onFocus: () => tick().then(() => composer?.focus()),
  })

  function dictationBusy() {
    // Establish Svelte dependencies for the controller state read below.
    dictationCommandPending
    dictationFinishing
    dictationPolishing
    dictationTransformPending
    dictation
    return dictationController.busy()
  }

  function invalidateDictationTransform() {
    dictationController.invalidateTransform()
  }

  function stopDictation(cancelled = false) {
    return dictationController.stop(cancelled)
  }

  function startDictation() {
    return dictationController.start()
  }

  function expectVoiceClick() {
    suppressVoiceClick = true
    clearTimeout(voiceClickTimer)
    voiceClickTimer = setTimeout(() => { suppressVoiceClick = false })
  }

  function clearPendingVoiceRelease() {
    clearTimeout(voiceReleaseTimer)
    voiceReleaseTimer = undefined
    voiceReleasePending = false
  }

  function activateVoice(source) {
    const activatedAt = Date.now()
    voiceActivationStartedAt = activatedAt
    voiceActivationSource = source
    if (handsFreeDictation) {
      void stopDictation()
      return true
    } else if (voiceReleasePending && (dictationRequested || isDictationActive(dictation)) && source === pendingVoiceActivationSource && activatedAt - pendingVoiceActivationAt <= handsFreeActivationDelay) {
      clearTimeout(voiceReleaseTimer)
      voiceReleaseTimer = undefined
      voiceReleasePending = false
      handsFreeDictation = true
    } else if (dictationRequested || isDictationActive(dictation)) {
      void stopDictation()
      return true
    } else void startDictation()
    return false
  }

  function releaseVoice(source, cancelled = false) {
    if (cancelled) {
      void stopDictation(true)
      return
    }
    if (handsFreeDictation) return
    if (source !== voiceActivationSource || Date.now() - voiceActivationStartedAt >= handsFreeActivationDelay) {
      void stopDictation()
      return
    }
    voiceReleasePending = true
    pendingVoiceActivationAt = voiceActivationStartedAt
    pendingVoiceActivationSource = source
    clearTimeout(voiceReleaseTimer)
    voiceReleaseTimer = setTimeout(() => {
      voiceReleaseTimer = undefined
      voiceReleasePending = false
      void stopDictation()
    }, handsFreeActivationDelay)
  }

  function voicePointerDown(event) {
    if (event.button !== 0 || voicePointerId !== undefined || voiceKey !== undefined) return
    event.preventDefault()
    expectVoiceClick()
    voicePointerId = event.pointerId
    event.currentTarget.setPointerCapture?.(event.pointerId)
    ignoreVoiceRelease = activateVoice('pointer')
  }

  function voicePointerEnd(event) {
    if (event.pointerId !== voicePointerId) return
    event.preventDefault()
    expectVoiceClick()
    voicePointerId = undefined
    if (ignoreVoiceRelease) {
      ignoreVoiceRelease = false
      return
    }
    releaseVoice('pointer', event.type === 'pointercancel')
  }

  function voiceKeyDown(event) {
    if (event.key !== ' ' && event.key !== 'Enter') return
    event.preventDefault()
    expectVoiceClick()
    if (!event.repeat) {
      if (voiceKey !== undefined || voicePointerId !== undefined) return
      voiceKey = event.key
      ignoreVoiceRelease = activateVoice('keyboard')
    }
  }

  function voiceKeyUp(event) {
    if (event.key !== voiceKey) return
    event.preventDefault()
    expectVoiceClick()
    voiceKey = undefined
    if (ignoreVoiceRelease) {
      ignoreVoiceRelease = false
      return
    }
    releaseVoice('keyboard')
  }

  function voiceClick() {
    if (suppressVoiceClick) {
      suppressVoiceClick = false
      clearTimeout(voiceClickTimer)
      return
    }
    const ignoreRelease = activateVoice('click')
    if (!ignoreRelease) releaseVoice('click')
  }

  function globalVoiceShortcut({ state }) {
    if (state === 'Pressed') {
      if (globalVoiceHeld || auth.name !== 'signed-in' || active || (dictationBusy() && !voiceReleasePending && !handsFreeDictation)) return
      globalVoiceHeld = true
      ignoreGlobalVoiceRelease = activateVoice('global')
    } else if (state === 'Released' && globalVoiceHeld) {
      globalVoiceHeld = false
      if (ignoreGlobalVoiceRelease) {
        ignoreGlobalVoiceRelease = false
        return
      }
      releaseVoice('global')
    }
  }

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

  function transformDictation(action) {
    return dictationController.transform(action)
  }

  function composerInput(event) {
    composerInputDraft = event.currentTarget.value
    if (eligibleDictation && event.currentTarget.value !== eligibleDictation.draft) invalidateDictationTransform()
  }

  // The polish overlay covers a textarea that scrolls once the draft passes the
  // ten-line cap; keep the two scrolled together so the underlined transcript
  // stays on the line it belongs to.
  function syncPolishPreviewScroll() {
    if (composer && polishPreview) polishPreview.scrollTop = composer.scrollTop
  }

  // §4: the input grows with the draft to a ten-line cap, then scrolls.
  // Measuring needs the textarea collapsed first, and every step of that
  // resizes the thread, which lets the browser clamp its scrollTop. Put the
  // transcript back where it was — at the bottom while it is pinned, otherwise
  // exactly where the reader left it — so growing the composer never scrolls it.
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
      // Past the cap a programmatic write — a streamed transcript, a transform
      // result — lands below the fold and the user watches their words vanish.
      // Typed input needs no help: the browser keeps the caret in view.
      if (reveal && capped) composer.scrollTop = composer.scrollHeight
    }
    if (thread) {
      const restored = pinned ? thread.scrollHeight - thread.clientHeight : threadScrollTop
      if (thread.scrollTop !== restored) thread.scrollTop = restored
      // The restore fires a scroll event; leave the follow state matching it so
      // handleThreadScroll does not read the correction as an upward scroll.
      lastScrollTop = thread.scrollTop
    }
    syncPolishPreviewScroll()
  }

  // Keyed off `draft` rather than the input event so every programmatic write
  // resizes too: streamed dictation transcripts, the polish and transform
  // results, the Esc restore in stopDictation(true), the Try again retry, and
  // send()/queue() clearing the draft back to the resting height. Untracked so
  // the follow state it reads cannot re-enter — resizing must answer to the
  // draft alone, never to a scroll already in flight.
  $effect(() => {
    draft
    polishPreview
    if (composer) untrack(() => {
      const typed = draft === composerInputDraft
      composerInputDraft = undefined
      syncComposerHeight(!typed)
    })
  })

  // The composer also rewraps when only its width changes, and most of those
  // never touch the window: ⌘J opening the artifact rail, the rail separator
  // being dragged or arrow-keyed, the sidebar collapsing. A height measured at
  // the old width would clip the draft with no scrollbar to reach it, so watch
  // the layout rather than the window. The action row is the box to observe:
  // it spans the same width as the input but is the one part of the composer
  // whose size we never set ourselves, so the callback cannot resize its own
  // target — observing the textarea makes Chromium report "ResizeObserver loop
  // completed with undelivered notifications" all through a rail drag.
  $effect(() => {
    if (!composerRow || typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(() => untrack(syncComposerHeight))
    observer.observe(composerRow)
    return () => observer.disconnect()
  })

  function streamingUnderline(node) {
    let mounted = true
    function measure() {
      const caret = node.querySelector('.caret')
      const rule = node.querySelector('.streaming-rule')
      if (!caret || !rule) return
      const geometry = streamingUnderlineGeometry({
        caretLeft: caret.offsetLeft,
        caretTop: caret.offsetTop,
        caretHeight: caret.offsetHeight,
      })
      if (!geometry) return
      rule.style.left = `${geometry.left}px`
      rule.style.top = `${geometry.top}px`
      rule.style.width = `${geometry.width}px`
    }

    function measureAfterRender() {
      tick().then(() => {
        if (mounted) measure()
      })
    }

    measureAfterRender()
    document.fonts?.ready?.then(() => {
      if (mounted) measure()
    })
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure)
    observer?.observe(node)
    return {
      update: measureAfterRender,
      destroy: () => {
        mounted = false
        observer?.disconnect()
      },
    }
  }

  function toggleReceipt(runId) {
    const next = new Set(expandedReceipts)
    next.has(runId) ? next.delete(runId) : next.add(runId)
    expandedReceipts = next
  }

  // The clipboard write runs first so it keeps the click's user activation. The
  // result is then published in two steps — clear, then set — so copying the same
  // reply twice still changes the live region's text and is announced again.
  async function copyResponse(run) {
    clearTimeout(copyTimer)
    const epoch = ++copyEpoch
    let result
    try {
      await navigator.clipboard.writeText(run.text ?? '')
      result = copyResult(run.id, true)
    } catch (_) {
      result = copyResult(run.id, false)
    }
    if (destroyed || epoch !== copyEpoch) return
    copy = null
    await tick()
    if (destroyed || epoch !== copyEpoch) return
    copy = result
    if (result.status !== 'copied') return
    copyTimer = setTimeout(() => {
      if (!destroyed && epoch === copyEpoch) copy = null
    }, COPY_CONFIRMATION_MS)
  }

  function scrollToLatest() {
    if (!thread) return
    pinned = true
    const behavior = window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth'
    thread.scrollTo({ top: thread.scrollHeight, behavior })
    lastScrollTop = thread.scrollHeight - thread.clientHeight
    hasContentBelow = false
  }

  function followNewContent() {
    if (!pinned) return
    tick().then(() => {
      if (!pinned || !thread) return
      thread.scrollTo({ top: thread.scrollHeight, behavior: 'auto' })
      lastScrollTop = thread.scrollHeight - thread.clientHeight
      hasContentBelow = false
    })
  }

  function handleThreadScroll() {
    const next = scrollFollowState({
      pinned,
      scrollTop: thread.scrollTop,
      scrollHeight: thread.scrollHeight,
      clientHeight: thread.clientHeight,
      lastScrollTop,
    })
    pinned = next.pinned
    lastScrollTop = next.lastScrollTop
    hasContentBelow = !pinned && thread.scrollHeight - thread.clientHeight - thread.scrollTop > 0
  }

  $effect(() => {
    messages
    followNewContent()
  })

  $effect(() => {
    if (auth.name !== 'signed-in' || onboarding.name !== 'complete') artifactRailOpen = false
  })

  $effect(() => {
    const inWorkspace = auth.name === 'signed-in' && onboarding.name === 'complete'
    if (inWorkspace && !wasInWorkspace && active?.phase !== 'resuming' && !dictationPolishing && composer) {
      wasInWorkspace = true
      composer.focus()
    } else if (!inWorkspace) {
      wasInWorkspace = false
    }
  })

  $effect(() => {
    const next = new Map(parallelTools)
    for (const message of messages) {
      if (message.role !== 'assistant') continue
      const running = (message.run.toolActivity ?? []).filter((tool) => tool.status === 'running')
      if (running.length > 1) {
        const grouped = new Set(next.get(message.run.id) ?? [])
        running.forEach((tool) => grouped.add(tool.effectId))
        next.set(message.run.id, [...grouped])
      }
    }
    if ([...next].some(([id, tools]) => tools.length !== (parallelTools.get(id)?.length ?? 0))) parallelTools = next
  })

  async function run(action) {
    const command = {
      status: 'auth_status',
      'sign-in': 'auth_sign_in',
      'sign-out': 'auth_sign_out',
    }[action]

    if (action === 'sign-in') auth = waitingState()
    try {
      const status = await tauri.invoke(command)
      auth = statusState(status)
      if (auth.name === 'signed-in') await loadHistory()
    } catch (err) {
      auth = errorState(action, err)
    }
  }

  async function loadHistory() {
    historyError = ''
    expandedReceipts = new Set()
    announcedRun = null
    try {
      const history = await tauri.invoke('chat_history')
      messages = historyMessages(history)
      pinned = true
      followNewContent()
    } catch (_) {
      historyError = 'Conversation history could not be restored. Try again.'
    }
  }

  onMount(() => {
    let pairingUnlisten
    window.__TAURI__?.event?.listen('attach-pairing-requested', async ({ payload: challenge }) => {
      const approve = await confirm(
        'Allow the CLI or VS Code to connect to this Muniment desktop session?',
        { title: 'Approve Muniment connection', kind: 'info' },
      )
      await tauri.invoke('attach_pairing_decide', { challenge, approve })
    }).then((stop) => {
      if (destroyed) stop()
      else pairingUnlisten = stop
    })
    if (tauri) {
      run('status')
      voiceShortcutManager.start()
    }
    window.__TAURI__?.event?.listen('chat-event', ({ payload }) => {
      if (!messages.some((message) => message.run?.id === payload.runId)) {
        buffered.set(payload.runId, [...(buffered.get(payload.runId) ?? []), payload])
        return
      }
      const current = messages.find((message) => message.run?.id === payload.runId)?.run
      const projected = applyChatEvent(current, payload)
      if (projected) messages = messages.map((message) => message.run?.id === projected.id ? { ...message, run: projected } : message)
      // Announce runs that were still in flight — including one restored mid-reply — plus
      // the run already being announced. A settled transcript stays silent.
      const settled = ['complete', 'cancelled', 'failed', 'interrupted'].includes(current?.phase)
      if (projected && (!settled || announcedRun?.id === payload.runId)) announcedRun = projected
      if (active?.id === payload.runId) active = projected && !['complete', 'cancelled', 'failed', 'interrupted'].includes(projected.phase) ? projected : null
    }).then((stop) => { unlisten = stop })
    const shortcuts = (event) => {
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
      const action = event.altKey && !event.ctrlKey && !event.metaKey ? dictationTransforms.find(({ key }) => `Digit${key}` === event.code) : undefined
      if (action && eligibleDictation && !dictationBusy()) {
        event.preventDefault()
        void transformDictation(action)
        return
      }
      if (event.key === 'Escape' && dictationTransformPending) {
        event.preventDefault()
        dictationController.cancelTransform()
        return
      }
      if (event.key === 'Escape' && (dictationRequested || isDictationActive(dictation) || dictationFinishing || dictationPolishing)) {
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
      })
    return () => {
      destroyed = true
      dictationController.cleanup()
      unlisten?.()
      pairingUnlisten?.()
      clearTimeout(voiceClickTimer)
      clearTimeout(voiceReleaseTimer)
      clearTimeout(copyTimer)
      stopDragDrop?.()
      globalVoiceHeld = false
      voiceShortcutManager.cleanup()
      document.removeEventListener('keydown', shortcuts)
      window.removeEventListener('resize', fitArtifactRail)
    }
  })

  async function send() {
    const prompt = draft.trim()
    if (!prompt || active || dictationBusy()) return
    invalidateDictationTransform()
    submitError = ''
    const submissionId = ++submissionSequence
    const userMessage = { role: 'user', text: prompt, attachments: [], submissionId }
    messages.push(userMessage)
    const pending = { id: 'pending', phase: 'thinking', text: '', receipt: null, prompt, submissionId }
    active = pending
    announcedRun = pending
    messages.push({ role: 'assistant', run: pending })
    followNewContent()
    try {
      const run = await tauri.invoke('chat_submit', {
        prompt,
        files: selectedFiles.map(({ path }) => ({ path })),
      })
      draft = ''
      selectedFiles = []
      messages = messages.map((message) => message.submissionId === submissionId ? { ...message, attachments: run.attachments ?? [] } : message)
      active = { ...pending, id: run.runId }
      const early = buffered.get(run.runId) ?? []
      const projected = applyBufferedChatEvents(active, early)
      buffered.delete(run.runId)
      messages = messages.map((message) => message.run?.submissionId === submissionId ? { ...message, run: projected } : message)
      announcedRun = projected
      active = ['complete', 'cancelled', 'failed', 'interrupted'].includes(projected.phase) ? null : projected
    } catch (error) {
      const failed = { ...pending, id: `rejected-${messages.length}`, phase: 'failed' }
      messages = messages.map((message) => message.run?.submissionId === submissionId ? { ...message, run: failed } : message)
      announcedRun = failed
      submitError = typeof error === 'string' ? error : 'The message could not be sent. Try again.'
      active = null
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

  async function cancel() {
    cancelError = ''
    try {
      await tauri.invoke('chat_cancel', { runId: active.id })
    } catch (_) {
      cancelError = 'Could not stop this reply. Try again.'
    }
  }

  async function resume(run) {
    if (active || dictationBusy() || !run.resumable || run.phase !== 'interrupted') return
    const resuming = { ...run, phase: 'resuming', resumeError: '' }
    active = resuming
    announcedRun = resuming
    messages = messages.map((message) => message.run?.id === run.id ? { ...message, run: resuming } : message)
    try {
      await tauri.invoke('chat_resume', { runId: run.id })
      const early = buffered.get(run.id) ?? []
      // Live events can arrive while invoke is still waiting for the durable
      // run.resumed transition. Reconcile from that newer projection instead
      // of restoring the stale, local `resuming` snapshot.
      const current = messages.find((message) => message.run?.id === run.id)?.run ?? resuming
      const projected = applyBufferedChatEvents(current, early)
      buffered.delete(run.id)
      messages = messages.map((message) => message.run?.id === run.id ? { ...message, run: projected } : message)
      announcedRun = projected
      active = ['complete', 'cancelled', 'failed', 'interrupted'].includes(projected.phase) ? null : projected
    } catch (error) {
      const interrupted = { ...run, phase: 'interrupted', resumeError: typeof error === 'string' ? error : 'This reply could not be resumed. Try again.' }
      messages = messages.map((message) => message.run?.id === run.id ? { ...message, run: interrupted } : message)
      announcedRun = interrupted
      active = null
    }
  }

  async function queue(delivery) {
    const message = draft.trim()
    if (!message || !active || active.id === 'pending' || dictationBusy()) return
    const runId = active.id
    queueError = ''
    try {
      await tauri.invoke('chat_queue', { runId, delivery, message })
      messages.push({ role: 'user', text: message })
      followNewContent()
      if (draft.trim() === message) draft = ''
    } catch (err) {
      queueError = typeof err === 'string' ? err : String(err)
    }
  }

  function keydown(event) {
    const action = composerAction(event, draft, active)
    if (action) {
      event.preventDefault()
      action === 'submit' ? send() : queue('steer')
    }
  }
</script>

<main>
  {#if auth.name !== 'signed-in' || onboarding.name !== 'complete'}
    <div class="lockup">
      <svg width="34" height="34" viewBox="0 0 48 48" role="img" aria-label="muniment">
        <path d={markD} stroke-width="4.5" />
      </svg>
      <span class="name">muniment</span>
    </div>
    <p class="meta">shell v{version}</p>
  {/if}

  {#if tauri}
    <Onboarding {tauri} bind:onboarding />
    {#if onboarding.name === 'complete'}
      {#if auth.name === 'signed-out'}
      <section class="auth-state">
        <p class="support">Sign in to continue to your workspace.</p>
        <button onclick={() => run('sign-in')}>Sign in</button>
      </section>
    {:else if auth.name === 'signing-in'}
      <section class="auth-state" aria-live="polite">
        <button disabled>Sign in</button>
        <p class="record">Waiting for the browser sign-in…</p>
      </section>
    {:else if auth.name === 'signed-in'}
      <section class="workspace" class:sidebar-collapsed={sidebarCollapsed} class:artifact-open={artifactRailOpen} class:artifact-resizing={artifactRailPointer !== undefined} style:--artifact-rail-width={`${artifactRailWidth}px`} bind:this={workspace}>
        {#if draggingFiles}<div class="drop-affordance" role="status"><strong>Drop files to add them</strong><span>Saved locally · supported images sent with first prompt</span></div>{/if}
        <header class="titlebar"><span class="thread-title">New thread</span><span class="thread-id">local · durable</span><span class="title-spacer"></span><button type="button" class="quiet" aria-controls="artifact-rail" aria-expanded={artifactRailOpen} aria-keyshortcuts={artifactShortcut} aria-label={`${artifactRailOpen ? 'Close' : 'Open'} artifact rail`} onclick={toggleArtifactRail}>Artifacts <kbd>{shortcutDisplayLabel(artifactShortcut)}</kbd></button></header>
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
          {#if !sidebarCollapsed}
            <p class="side-label">Threads</p>
            <button class="thread-row active-thread"><span></span>New thread</button>
          {/if}
          <button class="side-action home-settings" aria-label={sidebarCollapsed ? 'Home settings' : null} title={sidebarCollapsed ? 'Home settings' : null} onclick={() => { onboarding = onboardingSettingsState(onboarding) }}><svg class="side-icon" width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M4.5 10.5 12 4.75l7.5 5.75V19a1.5 1.5 0 0 1-1.5 1.5H6A1.5 1.5 0 0 1 4.5 19z" /><path d="M9.75 20.5v-5.75h4.5v5.75" /></svg>{#if !sidebarCollapsed}<span>Home settings</span>{/if}</button>
          {#if !sidebarCollapsed}
            <AccessPanel {tauri} subject={auth.subject} onSignOut={() => run('sign-out')} escapeBlocked={() => dictationRequested || isDictationActive(dictation)} voiceShortcut={globalVoiceShortcutValue} voiceShortcutChanging={globalVoiceChanging} onVoiceShortcutChange={changeVoiceShortcut} defaultVoiceShortcut={holdToTalkShortcut()} />
          {/if}
        </aside>
        <div class="thread-shell">
        <div class="thread" bind:this={thread} onscroll={handleThreadScroll}>
          {#if historyError}<p class="history-error" role="alert">{historyError} <button onclick={loadHistory}>Try again</button></p>{/if}
          {#if messages.length === 0}<p class="empty">Ask anything. Your org's routing decides which model answers.</p>{/if}
          {#each messages as message}
            {#if message.role === 'user'}
              <div class="user-turn">
                <div class="user-message">
                  {#if message.text}<p>{message.text}</p>{:else}<p class="missing-prompt">Prompt unavailable</p>{/if}
                  {#if message.attachments?.length}
                    <ul class="message-attachments" aria-label="Saved attachments">
                      {#each message.attachments as attachment}
                        <li><span>{attachment.displayName}</span><span>{formatByteSize(attachment.byteLength)}</span><strong>Saved locally · supported images sent with first prompt</strong></li>
                      {/each}
                    </ul>
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
              {:else}<p class="response-prose">{message.run.text}</p>{/if}
              {#if message.run.phase === 'failed'}<div class="run-error">Reply failed. <button disabled={dictationBusy()} onclick={() => { draft = message.run.prompt; send() }}>Try again</button></div>{/if}
              {#if message.run.phase === 'interrupted'}<div class="run-error" role={message.run.resumeError ? 'alert' : undefined}>{message.run.resumeError ?? 'Reply interrupted.'} {#if message.run.resumable}<button disabled={!!active || dictationBusy()} onclick={() => resume(message.run)}>Resume</button>{:else if message.run.prompt}<button disabled={dictationBusy()} onclick={() => { draft = message.run.prompt; send() }}>Try again</button>{/if}</div>{/if}
              {#if groupedTools.length}
                <div class="tool-card tool-group" role="group" aria-label={`Parallel tool activity: ${groupedTools.map((tool) => `${toolName(tool)} ${toolStatus(tool)}`).join(', ')}`}>
                  <div class="tool-group-title">Parallel tool activity</div>
                  {#each groupedTools as tool}
                    <div class:tool-running={toolStatus(tool) === 'running'} class:tool-failed={toolStatus(tool) === 'failed'} class="tool-row" aria-label={`${toolName(tool)}: ${toolStatus(tool)}`}>
                      <span class="tool-dot" aria-hidden="true"></span><span class="tool-name">{toolName(tool)}</span><span class="tool-status">{toolStatus(tool)}</span>
                    </div>
                  {/each}
                </div>
              {/if}
              {#each singleTools as tool}
                <div class:tool-running={toolStatus(tool) === 'running'} class:tool-failed={toolStatus(tool) === 'failed'} class="tool-card tool-row" role="status" aria-label={`${toolName(tool)}: ${toolStatus(tool)}`}>
                  <span class="tool-dot" aria-hidden="true"></span><span class="tool-name">{toolName(tool)}</span><span class="tool-status">{toolStatus(tool)}</span>
                </div>
              {/each}
              {#if message.run.phase === 'complete'}
                {@const summary = receiptSummary(message.run.receipt)}
                {#if summary.route !== null || summary.detail}
                  {@const expanded = expandedReceipts.has(message.run.id)}
                  {@const rows = receiptRows(message.run.receipt)}
                  <button class="provenance" aria-expanded={expanded} aria-label={`${expanded ? 'Collapse' : 'Expand'} receipt: ${receiptLabel(message.run.receipt)}`} onclick={() => toggleReceipt(message.run.id)}>{#if summary.route !== null}<span class="route-segment">{summary.route}</span>{/if}{summary.separator}{summary.detail}</button>
                  {#if expanded}
                    <dl class="receipt-record">
                      {#each rows as row}
                        <div><dt>{row.label}</dt><dd class:route-value={row.route}>{row.value}</dd></div>
                      {/each}
                    </dl>
                  {/if}
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
            <textarea class:polishing={dictationPolishing} bind:this={composer} bind:value={draft} oninput={composerInput} onkeydown={keydown} onscroll={syncPolishPreviewScroll} rows="2" placeholder={active?.phase === 'resuming' ? 'Resuming interrupted reply…' : 'Ask anything'} disabled={active?.phase === 'resuming'} readonly={dictationPolishing}></textarea>
            {#if dictationPolishing}
              <div class="polish-preview" bind:this={polishPreview} aria-hidden="true"><span data-testid="polish-draft">{appendTranscript(dictationDraftSnapshot, dictationTranscript).slice(0, -dictationTranscript.length)}</span><span class="polish-transcript" data-testid="polish-transcript">{dictationTranscript}</span></div>
            {/if}
          </div>
          {#if submitError}<p class="cancel-error" role="alert">{submitError}</p>{/if}
          {#if cancelError}<p class="cancel-error" role="alert">{cancelError}</p>{/if}
          {#if queueError}<p class="cancel-error" role="alert">{queueError}</p>{/if}
          {#if eligibleDictation}
            <div class="dictation-transforms" aria-label="Voice transforms">
              {#each dictationTransforms as action}
                <button type="button" aria-label={action.label} aria-keyshortcuts={action.shortcut} title={`${action.label} (Option+${action.key} / Alt+${action.key})`} disabled={dictationTransformPending} onclick={() => transformDictation(action)}><span>{action.label}</span><kbd>⌥{action.key}</kbd></button>
              {/each}
            </div>
          {/if}
          <div class="composer-row" bind:this={composerRow}>
            {#if dictationTransformPending}
              <span class="polish-status" role="status">Transforming on this device…</span>
            {:else if dictationPolishing}
              <span class="polish-status" role="status">Polishing on this device…</span>
            {:else if isDictationActive(dictation)}
              <span class="capture-status" role="status">
                <span class="capture-meter" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i></span>
                {dictation.state === 'starting' ? 'Starting local dictation…' : 'Listening on this device…'}
              </span>
            {:else}
              <span>{active?.phase === 'resuming' ? 'Reopening the existing secure session…' : active && active.id !== 'pending' ? '⏎ steers this reply · queue as follow-up' : 'Routing is automatic. Every reply carries its receipt.'}</span>
            {/if}
            <div class="composer-actions">
              <button type="button" class="quiet" aria-pressed={isDictationActive(dictation)} aria-keyshortcuts={ariaKeyShortcut(globalVoiceShortcutValue)} disabled={!!active || dictationFinishing || dictationPolishing || dictationTransformPending} onpointerdown={voicePointerDown} onpointerup={voicePointerEnd} onpointercancel={voicePointerEnd} onkeydown={voiceKeyDown} onkeyup={voiceKeyUp} onclick={voiceClick}>Voice</button>
              {#if !active}<button type="button" class="quiet" onclick={chooseFiles}>Add files</button>{/if}
              {#if active?.phase === 'resuming'}
                <button disabled>Resuming…</button>
              {:else if active && active.id !== 'pending'}
                <button class="quiet follow-up" disabled={!draft.trim()} onclick={() => queue('followUp')}>Queue follow-up</button>
                <button onclick={cancel}>Stop</button>
                <button class="primary" disabled={!draft.trim()} onclick={() => queue('steer')}>Send</button>
              {:else if !active}<button class="primary" disabled={!draft.trim() || dictationBusy()} onclick={send}>Send</button>{/if}
            </div>
          </div>
          {#if dictationError}<div class="dictation-error" role="alert">{dictationError}</div>{/if}
          {#if globalVoiceError}<div class="dictation-error" role="alert">The system-wide voice shortcut is unavailable. Voice remains available from the button.</div>{/if}
        </div>
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

<style>
  main {
    min-height: 100vh;
    display: grid;
    place-content: center;
    justify-items: center;
  }

  /* Lockup (§1.8): mark at rest — static, ink — beside the wordmark,
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
  .composer-actions .primary:disabled { background: var(--faint); border-color: var(--border); color: var(--muted); }

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

  button:hover:not(:disabled) {
    border-color: var(--muted);
  }

  button:disabled {
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
  .drop-affordance { position: fixed; z-index: 4; inset: 52px 0 0 260px; display: grid; place-content: center; gap: 5px; background: color-mix(in srgb, var(--paper) 92%, transparent); border: 1px dashed var(--muted); color: var(--ink); text-align: center; pointer-events: none; }
  .drop-affordance span { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .titlebar { grid-area: title; display: flex; align-items: center; padding: 0 18px 0 278px; border-bottom: 1px solid var(--border); background: var(--surface); transition: padding-left 180ms ease; }
  .thread-title { font-weight: 600; }
  .thread-id, kbd { margin-left: 10px; color: var(--muted); font: var(--text-12) var(--font-mono); }
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
  .side-action span { flex: 1; }
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
  .response { margin: 0 0 34px; }
  .response-prose { max-width: 92%; white-space: pre-wrap; }
  .streaming { position: relative; }
  .streaming-rule { position: absolute; height: 2px; background: var(--signal); pointer-events: none; }
  .caret { display: inline-block; height: 1em; border-right: 2px solid var(--signal); margin-left: 2px; vertical-align: -2px; animation: blink 800ms step-end infinite; }
  .thinking { display: flex; align-items: center; gap: 9px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .thinking path { fill: var(--signal); animation: breathe 1.8s ease-in-out infinite; }
  .tool-card { margin-top: 8px; padding: 8px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); font: var(--text-13) var(--font-mono); }
  .tool-row { display: flex; align-items: center; gap: 8px; min-height: 20px; }
  .tool-group-title { margin-bottom: 4px; color: var(--muted); }
  .tool-group .tool-row + .tool-row { margin-top: 4px; }
  .tool-dot { width: 7px; height: 7px; flex: 0 0 auto; border-radius: 50%; background: currentColor; }
  .tool-name { min-width: 0; overflow-wrap: anywhere; }
  .tool-status { margin-left: auto; }
  .tool-running { color: var(--signal); }
  .tool-running .tool-dot { animation: tool-pulse 1.4s ease-in-out infinite; }
  .tool-failed .tool-status::before { content: 'error · '; }
  /* §2.2 mono 11.5px; §1.4 records line up their figures. The shorthand resets
     font-variant-numeric, so tabular-nums follows it. */
  .provenance { display: block; margin-top: 10px; padding: 0; border: 0; background: transparent; color: var(--muted); font: var(--text-provenance)/1.45 var(--font-mono); font-variant-numeric: tabular-nums; text-align: left; }
  .provenance:hover:not(:disabled) { color: var(--ink); }
  /* §1.2 permits --signal on the route segment only. */
  .provenance .route-segment { color: var(--signal); }
  .receipt-record { width: fit-content; min-width: 240px; margin: 8px 0 0; padding: 8px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); color: var(--muted); font-size: var(--text-12); }
  .receipt-record div { display: grid; grid-template-columns: 88px minmax(0, 1fr); gap: 12px; }
  .receipt-record dd { margin: 0; font-family: var(--font-mono); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .receipt-record .route-value { color: var(--signal); }
  /* §3.2: hover or focus reveals the row. Only opacity carries the reveal — the row
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
  .run-error button { padding: 2px 6px; }
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
  textarea.polishing { color: transparent; caret-color: transparent; }
  /* overflow-wrap matches the textarea's UA style so a single long token breaks
     on the same character in both layers. */
  .polish-preview { position: absolute; inset: 0; overflow: hidden; pointer-events: none; white-space: pre-wrap; overflow-wrap: break-word; color: var(--ink); font: inherit; }
  .polish-transcript { text-decoration-line: underline; text-decoration-color: var(--signal); text-decoration-thickness: 2px; text-underline-offset: 3px; }
  .dictation-transforms { display: flex; flex-wrap: wrap; gap: 5px; margin: 7px 0; }
  /* These chips appear after the polish flash has settled to ink, and §2.4 lets
     signal touch the composer only for the flash itself — so the group keeps the
     base button's ink-on-hairline treatment at chip radius. */
  .dictation-transforms button { display: inline-flex; align-items: center; gap: 7px; padding: 3px 7px; border-radius: var(--radius-chip); background: transparent; font: var(--text-12) var(--font-mono); }
  .dictation-transforms button:hover:not(:disabled) { background: var(--faint); }
  .dictation-transforms kbd { color: var(--muted); font: inherit; }
  /* The input no longer keeps a spare empty row once it grows, so the action
     row carries the gap itself — the owner mockup's 8px .comprow rhythm. */
  .composer-row { display: flex; justify-content: space-between; align-items: center; margin-top: 8px; color: var(--muted); font-size: var(--text-12); }
  .composer-actions { display: flex; align-items: center; gap: 6px; }
  .capture-status { display: flex; align-items: center; gap: 8px; font-family: var(--font-mono); }
  .capture-meter { height: 14px; display: flex; align-items: center; gap: 2px; }
  .capture-meter i { width: 2px; height: 6px; background: var(--muted); animation: capture 900ms ease-in-out infinite alternate; }
  .capture-meter i:nth-child(2), .capture-meter i:nth-child(4) { height: 10px; animation-delay: -300ms; }
  .capture-meter i:nth-child(3) { height: 14px; animation-delay: -600ms; }
  .polish-status { color: var(--signal); font-family: var(--font-mono); text-decoration: underline 2px; text-underline-offset: 3px; }
  .dictation-error { margin-top: 7px; color: var(--muted); font: var(--text-12) var(--font-mono); }
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
  @media (prefers-reduced-motion: reduce) {
    /* Unlike the blanket duration rule, removing this animation keeps the meter
       at its full-height resting state instead of the keyframe's 55% endpoint. */
    .capture-meter i { animation: none; }
    .thinking path { animation: none; }
  }
</style>
