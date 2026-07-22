<script>
  import { onMount, tick } from 'svelte'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { confirm, open } from '@tauri-apps/plugin-dialog'
  import { register, unregister } from '@tauri-apps/plugin-global-shortcut'

  import AccessPanel from './lib/AccessPanel.svelte'
  import { bootState, errorState, statusState, waitingState } from './lib/auth-state.js'
  import { ringPath } from './lib/mark.js'
  import { applyBufferedChatEvents, applyChatEvent, composerAction, formatByteSize, historyMessages, receiptParts, receiptRows, toolName, toolStatus } from './lib/chat-state.js'
  import { appendTranscript, dictationTransforms, handsFreeActivationDelay, holdToTalkShortcut, isDictationActive } from './lib/dictation-state.js'
  import { onboardingCancelSettingsState, onboardingConfirmedState, onboardingConfirmingState, onboardingErrorState, onboardingExtractingState, onboardingExtractionErrorState, onboardingExtractionState, onboardingFinalizingState, onboardingImportChoiceState, onboardingLoadingState, onboardingPathState, onboardingPreviewErrorState, onboardingPreviewingState, onboardingPreviewState, onboardingReturnToArchiveReviewState, onboardingSelectionState, onboardingSettingsState, onboardingStatusState, onboardingTriageConfirmedState, onboardingTriageErrorState, onboardingTriageReportState, onboardingTriagingState, requiredModelLoadingState, requiredModelPollActive, requiredModelProgress } from './lib/onboarding-state.js'
  import { scrollFollowState } from './lib/scroll-follow.js'

  const markD = ringPath()
  const version = __APP_VERSION__

  const tauri = window.__TAURI__?.core
  let auth = $state(bootState)
  let draft = $state('')
  let selectedFiles = $state([])
  let submitError = $state('')
  let messages = $state([])
  let active = $state(null)
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
  let dictationTimer
  let dictationPollEpoch = 0
  let dictationUnlisten
  let dictationCommandPending = $state(false)
  let dictationRequested = false
  let dictationCancelled = true
  let dictationDraftSnapshot = $state('')
  let dictationTranscript = $state('')
  let dictationCaptureEpoch = 0
  let dictationCompletionEpoch
  let dictationCompletionTimer
  let dictationFinishing = $state(false)
  let dictationPolishEpoch
  let dictationPolishing = $state(false)
  let dictationTransformEpoch = 0
  let dictationTransformPending = $state(false)
  let dictationTransformPendingEpoch
  let eligibleDictation = $state(null)
  let eligibleDictationTimer
  let eligibleDictationTimerEpoch = 0
  let suppressVoiceClick = false
  let voiceClickTimer
  let voiceReleaseTimer
  let voiceReleasePending = false
  let handsFreeDictation = false
  let ignoreVoiceRelease = false
  let ignoreGlobalVoiceRelease = false
  let voicePointerId
  let voiceKey
  let globalVoiceHeld = false
  let globalVoiceRegistered = false
  let globalVoiceError = $state(false)
  let composer = $state()
  let onboarding = $state(onboardingLoadingState)
  let requiredModel = $state(requiredModelLoadingState)
  let modelProgress = $derived(requiredModelProgress(requiredModel))
  let requiredModelTimer
  let requiredModelPollEpoch = 0
  let onboardingPreviewSequence = 0
  let destroyed = false
  const dictationTranscriptQuietPeriod = 25

  async function loadOnboarding() {
    try {
      onboarding = onboardingStatusState(await tauri.invoke('home_status'))
      if (onboarding.name !== 'complete') pollRequiredModel()
    } catch (error) {
      onboarding = { name: 'load-error', homePath: '', error: typeof error === 'string' ? error : 'Onboarding could not be loaded.' }
    }
  }

  async function pollRequiredModel() {
    if (destroyed || onboarding.name === 'complete') return
    const epoch = ++requiredModelPollEpoch
    clearTimeout(requiredModelTimer)
    try {
      const status = await tauri.invoke('required_model_acquisition_status')
      if (destroyed || epoch !== requiredModelPollEpoch || onboarding.name === 'complete') return
      requiredModel = status
      if (requiredModelPollActive(status)) {
        requiredModelTimer = setTimeout(pollRequiredModel, 1000)
      }
    } catch (_) {
      if (destroyed || epoch !== requiredModelPollEpoch || onboarding.name === 'complete') return
      requiredModel = { ...requiredModelLoadingState, status: { state: 'failed' } }
    }
  }

  function stopRequiredModelPolling() {
    requiredModelPollEpoch += 1
    clearTimeout(requiredModelTimer)
    requiredModelTimer = undefined
  }

  async function chooseHome() {
    try {
      const picked = await open({ directory: true, multiple: false, defaultPath: onboarding.homePath })
      if (typeof picked === 'string') onboarding = onboardingPathState(onboarding, picked)
    } catch (_) {
      onboarding = onboardingErrorState(onboarding, 'The folder picker could not be opened. Try again.')
    }
  }

  async function confirmHome() {
    if (!onboarding.savedHomePath) {
      onboarding = onboardingImportChoiceState(onboarding)
      return
    }
    const pending = onboardingConfirmingState(onboarding)
    onboarding = pending
    try {
      onboarding = onboardingConfirmedState(pending, await tauri.invoke('home_confirm', { homePath: pending.homePath }))
      if (onboarding.name === 'complete') stopRequiredModelPolling()
    } catch (error) {
      onboarding = onboardingErrorState(pending, typeof error === 'string' ? error : undefined)
    }
  }

  async function chooseImportArchive() {
    let picked
    try {
      picked = await open({ multiple: false, directory: false, filters: [{ name: 'ZIP archives', extensions: ['zip'] }] })
    } catch (_) {
      onboarding = { ...onboarding, error: 'The file picker could not be opened. Try again.' }
      return
    }
    if (typeof picked !== 'string') return
    const previewId = ++onboardingPreviewSequence
    const pending = onboardingPreviewingState(onboarding, picked)
    onboarding = pending
    try {
      const manifest = await tauri.invoke('onboarding_import_preview', { archivePath: picked })
      if (previewId === onboardingPreviewSequence) onboarding = onboardingPreviewState(pending, manifest)
    } catch (error) {
      if (previewId === onboardingPreviewSequence) onboarding = onboardingPreviewErrorState(pending, error)
    }
  }

  async function skipImport() {
    onboardingPreviewSequence += 1
    const pending = onboardingFinalizingState(onboarding)
    onboarding = pending
    try {
      onboarding = onboardingConfirmedState(pending, await tauri.invoke('home_confirm', { homePath: pending.homePath }))
      if (onboarding.name === 'complete') stopRequiredModelPolling()
    } catch (error) {
      onboarding = onboardingErrorState(pending, typeof error === 'string' ? error : undefined)
    }
  }

  function selectImportEntry(entryName, selected) {
    onboarding = onboardingSelectionState(onboarding, entryName, selected)
  }

  async function extractImportSelection() {
    if (onboarding.name !== 'reviewing' || onboarding.selectedNames.length === 0) return
    const extractionId = ++onboardingPreviewSequence
    const pending = onboardingExtractingState(onboarding)
    onboarding = pending
    try {
      const extracted = await tauri.invoke('onboarding_import_extract', {
        archivePath: pending.archivePath,
        selectedNames: pending.selectedNames,
      })
      if (extractionId === onboardingPreviewSequence) onboarding = onboardingExtractionState(pending, extracted)
    } catch (error) {
      if (extractionId === onboardingPreviewSequence) onboarding = onboardingExtractionErrorState(pending, error)
    }
  }

  async function generateTriageReport() {
    if (!requiredModel.aiFeaturesAvailable || !['pre-triage', 'triage-error'].includes(onboarding.name)) return
    const triageId = ++onboardingPreviewSequence
    const pending = onboardingTriagingState(onboarding)
    onboarding = pending
    try {
      const response = await tauri.invoke('onboarding_triage', { entries: pending.extractedEntries })
      if (triageId === onboardingPreviewSequence) onboarding = onboardingTriageReportState(pending, response)
    } catch (error) {
      if (triageId === onboardingPreviewSequence) onboarding = onboardingTriageErrorState(pending, error)
    }
  }

  function returnToArchiveReview() {
    onboardingPreviewSequence += 1
    onboarding = onboardingReturnToArchiveReviewState(onboarding)
  }

  function stopDictationPolling() {
    clearTimeout(dictationTimer)
    dictationTimer = undefined
  }

  function invalidateDictationPolls() {
    dictationPollEpoch += 1
    stopDictationPolling()
  }

  function dictationBusy() {
    return dictationCommandPending || dictationFinishing || dictationPolishing || dictationTransformPending || isDictationActive(dictation)
  }

  function invalidateDictationTransform() {
    dictationTransformEpoch += 1
    eligibleDictationTimerEpoch += 1
    clearTimeout(eligibleDictationTimer)
    eligibleDictationTimer = undefined
    eligibleDictation = null
  }

  function offerDictationTransforms(eligible) {
    const timerEpoch = ++eligibleDictationTimerEpoch
    clearTimeout(eligibleDictationTimer)
    eligibleDictation = eligible
    eligibleDictationTimer = setTimeout(() => {
      if (timerEpoch === eligibleDictationTimerEpoch) invalidateDictationTransform()
    }, 6000)
  }

  async function listenForDictation(epoch) {
    dictationUnlisten?.()
    dictationUnlisten = undefined
    const stop = await window.__TAURI__?.event?.listen('dictation-event', ({ payload }) => {
      if (epoch !== dictationCaptureEpoch || payload.type !== 'transcript' || dictationCancelled || (!isDictationActive(dictation) && !dictationFinishing)) return
      dictationTranscript = appendTranscript(dictationTranscript, payload.text)
      draft = appendTranscript(dictationDraftSnapshot, dictationTranscript)
      if (dictationFinishing) waitForDictationTranscriptQuiet(epoch)
    })
    if (!stop) return
    if (destroyed || epoch !== dictationCaptureEpoch) stop()
    else dictationUnlisten = stop
  }

  function completeDictation(epoch) {
    if (destroyed || epoch !== dictationCaptureEpoch || epoch !== dictationCompletionEpoch) return
    dictationCompletionEpoch = undefined
    dictationFinishing = false
    if (!dictationCancelled) {
      dictationPolishEpoch = epoch
      void polishDictation(epoch)
    }
  }

  function waitForDictationTranscriptQuiet(epoch) {
    clearTimeout(dictationCompletionTimer)
    dictationCompletionTimer = setTimeout(() => completeDictation(epoch), dictationTranscriptQuietPeriod)
  }

  function finishDictation(epoch) {
    clearTimeout(dictationCompletionTimer)
    dictationCompletionTimer = setTimeout(() => waitForDictationTranscriptQuiet(epoch))
  }

  async function polishDictation(epoch) {
    if (dictationCancelled || epoch !== dictationCaptureEpoch || epoch !== dictationPolishEpoch) return
    const transcript = dictationTranscript
    if (!transcript.trim()) {
      dictationPolishEpoch = undefined
      return
    }
    dictationPolishing = true
    dictationError = ''
    const verbatimDraft = appendTranscript(dictationDraftSnapshot, transcript)
    try {
      const polished = await tauri.invoke('dictation_polish', { transcript })
      if (destroyed || dictationCancelled || epoch !== dictationCaptureEpoch || epoch !== dictationPolishEpoch) return
      if (draft === verbatimDraft && polished.trim()) {
        draft = appendTranscript(dictationDraftSnapshot, polished)
        offerDictationTransforms({ epoch, snapshot: dictationDraftSnapshot, segment: polished, draft })
      }
    } catch (_) {
      if (destroyed || dictationCancelled || epoch !== dictationCaptureEpoch || epoch !== dictationPolishEpoch) return
      dictationError = 'Polishing is unavailable. You can edit or send the captured text.'
    } finally {
      if (epoch === dictationPolishEpoch) {
        dictationPolishEpoch = undefined
        dictationPolishing = false
      }
    }
  }

  function applyDictationStatus(status) {
    dictation = status
    if (status.state === 'modelNotInstalled' || status.state === 'failed') {
      handsFreeDictation = false
      dictationError = status.message
    } else dictationError = ''
    if (!isDictationActive(status)) stopDictationPolling()
    if (status.state === 'stopped' && dictationCompletionEpoch !== undefined) {
      if (dictationCancelled) completeDictation(dictationCompletionEpoch)
      else {
        dictationFinishing = true
        finishDictation(dictationCompletionEpoch)
      }
    }
  }

  function pollDictation() {
    stopDictationPolling()
    const epoch = dictationPollEpoch
    dictationTimer = setTimeout(async () => {
      if (destroyed || epoch !== dictationPollEpoch || !isDictationActive(dictation)) return
      try {
        const status = await tauri.invoke('dictation_status')
        if (destroyed || epoch !== dictationPollEpoch) return
        applyDictationStatus(status)
      } catch (error) {
        if (destroyed || epoch !== dictationPollEpoch) return
        dictationError = typeof error === 'string' ? error : 'Dictation status could not be checked.'
      }
      if (!destroyed && epoch === dictationPollEpoch && isDictationActive(dictation)) pollDictation()
    }, 100)
  }

  async function stopDictation(cancelled = false) {
    clearTimeout(voiceReleaseTimer)
    voiceReleaseTimer = undefined
    voiceReleasePending = false
    handsFreeDictation = false
    dictationRequested = false
    invalidateDictationPolls()
    if (cancelled) {
      dictationCancelled = true
      dictationPolishEpoch = undefined
      dictationPolishing = false
      voicePointerId = undefined
      voiceKey = undefined
      draft = dictationDraftSnapshot
      tick().then(() => composer?.focus())
    }
    if (dictationCommandPending || !isDictationActive(dictation)) return
    dictationCompletionEpoch = dictationCaptureEpoch
    dictationFinishing = true
    dictationCommandPending = true
    dictationError = ''
    try {
      const status = await tauri.invoke('dictation_stop')
      if (destroyed) return
      applyDictationStatus(status)
      if (isDictationActive(dictation)) pollDictation()
    } catch (error) {
      if (destroyed) return
      dictationFinishing = false
      dictationError = typeof error === 'string' ? error : 'Dictation could not be stopped.'
      pollDictation()
    } finally {
      dictationCommandPending = false
    }
  }

  async function startDictation() {
    if (active || dictationBusy() || dictationRequested) return
    if (isDictationActive(dictation)) {
      await stopDictation()
      return
    }
    invalidateDictationPolls()
    dictationRequested = true
    invalidateDictationTransform()
    dictationCancelled = false
    dictationDraftSnapshot = draft
    dictationTranscript = ''
    dictationCaptureEpoch += 1
    dictationCompletionEpoch = undefined
    dictationFinishing = false
    dictationPolishEpoch = undefined
    dictationCommandPending = true
    dictationError = ''
    dictation = { state: 'starting' }
    try {
      await listenForDictation(dictationCaptureEpoch)
      if (destroyed || dictationCancelled) return
      const status = await tauri.invoke('dictation_start')
      if (destroyed) return
      applyDictationStatus(status)
      if (!dictationRequested && isDictationActive(dictation)) {
        dictationCommandPending = false
        await stopDictation()
      } else if (isDictationActive(dictation)) pollDictation()
    } catch (error) {
      if (destroyed) return
      handsFreeDictation = false
      dictation = { state: 'failed' }
      dictationError = typeof error === 'string' ? error : 'Dictation could not be started.'
      dictationRequested = false
    } finally {
      dictationCommandPending = false
    }
  }

  function expectVoiceClick() {
    suppressVoiceClick = true
    clearTimeout(voiceClickTimer)
    voiceClickTimer = setTimeout(() => { suppressVoiceClick = false })
  }

  function activateVoice() {
    if (handsFreeDictation) {
      void stopDictation()
      return true
    } else if (voiceReleasePending) {
      clearTimeout(voiceReleaseTimer)
      voiceReleaseTimer = undefined
      voiceReleasePending = false
      handsFreeDictation = true
    } else void startDictation()
    return false
  }

  function releaseVoice(cancelled = false) {
    if (cancelled) {
      void stopDictation(true)
      return
    }
    if (handsFreeDictation) return
    voiceReleasePending = true
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
    ignoreVoiceRelease = activateVoice()
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
    releaseVoice(event.type === 'pointercancel')
  }

  function voiceKeyDown(event) {
    if (event.key !== ' ' && event.key !== 'Enter') return
    event.preventDefault()
    expectVoiceClick()
    if (!event.repeat) {
      if (voiceKey !== undefined || voicePointerId !== undefined) return
      voiceKey = event.key
      ignoreVoiceRelease = activateVoice()
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
    releaseVoice()
  }

  function voiceClick() {
    if (suppressVoiceClick) {
      suppressVoiceClick = false
      clearTimeout(voiceClickTimer)
      return
    }
    dictationRequested || isDictationActive(dictation) ? stopDictation() : startDictation()
  }

  function globalVoiceShortcut({ state }) {
    if (state === 'Pressed') {
      if (globalVoiceHeld || auth.name !== 'signed-in' || active || (dictationBusy() && !voiceReleasePending && !handsFreeDictation)) return
      globalVoiceHeld = true
      ignoreGlobalVoiceRelease = activateVoice()
    } else if (state === 'Released' && globalVoiceHeld) {
      globalVoiceHeld = false
      if (ignoreGlobalVoiceRelease) {
        ignoreGlobalVoiceRelease = false
        return
      }
      releaseVoice()
    }
  }

  async function transformDictation(action) {
    const eligible = eligibleDictation
    if (!eligible || dictationBusy() || draft !== eligible.draft) return
    const operation = ++dictationTransformEpoch
    clearTimeout(eligibleDictationTimer)
    eligibleDictationTimer = undefined
    dictationTransformPending = true
    dictationTransformPendingEpoch = operation
    dictationError = ''
    try {
      const transformed = await tauri.invoke('dictation_transform', { transform: action.transform, transcript: eligible.segment })
      if (destroyed || operation !== dictationTransformEpoch || draft !== eligible.draft) return
      if (!transformed.trim()) throw new Error('empty transform')
      draft = appendTranscript(eligible.snapshot, transformed)
      offerDictationTransforms({ ...eligible, segment: transformed, draft })
    } catch (_) {
      if (destroyed || operation !== dictationTransformEpoch) return
      dictationError = 'That voice transform is unavailable. Your text is unchanged; try again.'
      offerDictationTransforms(eligible)
    } finally {
      if (!destroyed && operation === dictationTransformPendingEpoch) {
        dictationTransformPending = false
        dictationTransformPendingEpoch = undefined
      }
      if (!destroyed) tick().then(() => composer?.focus())
    }
  }

  function composerInput(event) {
    if (eligibleDictation && event.currentTarget.value !== eligibleDictation.draft) invalidateDictationTransform()
  }

  function toggleReceipt(runId) {
    const next = new Set(expandedReceipts)
    next.has(runId) ? next.delete(runId) : next.add(runId)
    expandedReceipts = next
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
      loadOnboarding()
      run('status')
      register(holdToTalkShortcut(), globalVoiceShortcut).then(() => {
        globalVoiceRegistered = true
        if (destroyed) void unregister(holdToTalkShortcut()).catch(() => {})
      }).catch(() => {
        if (!destroyed) globalVoiceError = true
      })
    }
    window.__TAURI__?.event?.listen('chat-event', ({ payload }) => {
      if (!messages.some((message) => message.run?.id === payload.runId)) {
        buffered.set(payload.runId, [...(buffered.get(payload.runId) ?? []), payload])
        return
      }
      const current = messages.find((message) => message.run?.id === payload.runId)?.run
      const projected = applyChatEvent(current, payload)
      if (projected) messages = messages.map((message) => message.run?.id === projected.id ? { ...message, run: projected } : message)
      if (active?.id === payload.runId) active = projected && !['complete', 'cancelled', 'failed', 'interrupted'].includes(projected.phase) ? projected : null
    }).then((stop) => { unlisten = stop })
    const shortcuts = (event) => {
      const action = event.altKey && !event.ctrlKey && !event.metaKey ? dictationTransforms.find(({ key }) => `Digit${key}` === event.code) : undefined
      if (action && eligibleDictation && !dictationBusy()) {
        event.preventDefault()
        void transformDictation(action)
        return
      }
      if (event.key === 'Escape' && dictationTransformPending) {
        event.preventDefault()
        invalidateDictationTransform()
        dictationTransformPending = false
        dictationTransformPendingEpoch = undefined
        dictationError = ''
        tick().then(() => composer?.focus())
        return
      }
      if (event.key === 'Escape' && (dictationRequested || isDictationActive(dictation) || dictationFinishing || dictationPolishing)) {
        event.preventDefault()
        stopDictation(true)
        return
      }
    }
    document.addEventListener('keydown', shortcuts)
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
      invalidateDictationTransform()
      stopRequiredModelPolling()
      unlisten?.()
      dictationUnlisten?.()
      pairingUnlisten?.()
      stopDictationPolling()
      clearTimeout(dictationCompletionTimer)
      clearTimeout(voiceClickTimer)
      clearTimeout(voiceReleaseTimer)
      stopDragDrop?.()
      globalVoiceHeld = false
      if (globalVoiceRegistered) void unregister(holdToTalkShortcut()).catch(() => {})
      document.removeEventListener('keydown', shortcuts)
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
    const pending = { id: 'pending', phase: 'thinking', text: '', receipt: null, prompt }
    active = pending
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
      messages = messages.map((message) => message.run === pending ? { ...message, run: projected } : message)
      active = ['complete', 'cancelled', 'failed', 'interrupted'].includes(projected.phase) ? null : projected
    } catch (error) {
      const failed = { ...pending, id: `rejected-${messages.length}`, phase: 'failed' }
      messages = messages.map((message) => message.run === pending ? { ...message, run: failed } : message)
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
      active = ['complete', 'cancelled', 'failed', 'interrupted'].includes(projected.phase) ? null : projected
    } catch (error) {
      const interrupted = { ...run, phase: 'interrupted', resumeError: typeof error === 'string' ? error : 'This reply could not be resumed. Try again.' }
      messages = messages.map((message) => message.run?.id === run.id ? { ...message, run: interrupted } : message)
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

<main class:signed-frame={auth.name === 'signed-in'}>
  <div class="lockup">
    <svg width="34" height="34" viewBox="0 0 48 48" role="img" aria-label="muniment">
      <path d={markD} stroke-width="4.5" />
    </svg>
    <span class="name">muniment</span>
  </div>
  <p class="meta">shell v{version}</p>

  {#if tauri}
    {#if onboarding.name !== 'complete'}
      <section class="onboarding" aria-labelledby="onboarding-title">
        <p class="eyebrow">{onboarding.savedHomePath ? 'Home settings' : 'First-run setup'}</p>
        <h1 id="onboarding-title">{['pre-triage', 'triaging', 'triage-error'].includes(onboarding.name) ? 'Create your local proposal' : ['triage-review', 'triage-confirmed'].includes(onboarding.name) ? 'Review your onboarding proposal' : ['import-choice', 'previewing', 'reviewing', 'extracting', 'finalizing'].includes(onboarding.name) ? 'Review an assistant export' : 'Choose your Muniment Home'}</h1>
        {#if !onboarding.savedHomePath}
          <aside class="model-status" aria-labelledby="model-status-title">
            <div class="model-status-heading">
              <span id="model-status-title">Local AI</span>
              <strong>{requiredModel.aiFeaturesAvailable ? 'Ready' : requiredModel.status?.state === 'failed' || requiredModel.status?.state === 'cancelled' ? requiredModel.retryingInBackground ? 'Retrying in background' : 'Setup unavailable' : requiredModel.status?.state === 'installing' ? 'Downloading' : 'Starting setup'}</strong>
            </div>
            <p class="model-status-copy" aria-live="polite">{requiredModel.aiFeaturesAvailable ? 'Local proposal generation is ready.' : requiredModel.status?.state === 'failed' || requiredModel.status?.state === 'cancelled' ? requiredModel.retryingInBackground ? 'The download did not finish. Muniment will keep retrying in the background.' : 'Local AI setup could not finish. You can continue setting up your Home.' : 'The required model is being prepared in the background. You can continue setting up your Home.'}</p>
            {#if modelProgress.total > 0}
              <div class="model-progress" role="progressbar" aria-label="Required local AI model download" aria-valuemin="0" aria-valuemax={modelProgress.total} aria-valuenow={modelProgress.downloaded}>
                <span style={`width: ${modelProgress.downloaded / modelProgress.total * 100}%`}></span>
              </div>
              <p class="model-progress-copy">{formatByteSize(modelProgress.downloaded)} of {formatByteSize(modelProgress.total)}</p>
            {/if}
          </aside>
        {/if}
        {#if onboarding.name === 'loading'}
          <p class="support" role="status">Finding your Documents folder…</p>
        {:else if ['choosing', 'confirming', 'settings', 'confirming-settings'].includes(onboarding.name)}
          <p class="support">Your memory stays in plain Markdown files in a folder you control. Muniment will create four visible folders inside it.</p>
          <div class="path-card">
            <span class="path-label">Home location</span>
            <strong data-testid="onboarding-home-path">{onboarding.homePath}</strong>
            <button data-testid="onboarding-picker" onclick={chooseHome} disabled={onboarding.name.startsWith('confirming')}>Choose folder…</button>
          </div>
          <p class="folder-preview"><span>memory/</span><span>agents/</span><span>projects/</span><span>sessions/</span></p>
          <div class="onboarding-footer">
            {#if onboarding.savedHomePath}<button data-testid="onboarding-cancel" onclick={() => { onboarding = onboardingCancelSettingsState(onboarding) }} disabled={onboarding.name === 'confirming-settings'}>Cancel</button>{:else}<span class="privacy-note">Plain Markdown · stored locally</span>{/if}
            <button data-testid="onboarding-confirm" class="primary" onclick={confirmHome} disabled={onboarding.name.startsWith('confirming') || !onboarding.homePath}>{onboarding.name.startsWith('confirming') ? 'Creating Home…' : 'Confirm and continue'}</button>
          </div>
          {#if onboarding.error}<p class="onboarding-error" role="alert">{onboarding.error}</p>{/if}
        {:else if ['import-choice', 'previewing', 'reviewing', 'extracting', 'finalizing'].includes(onboarding.name)}
          <p class="support">Optionally choose one assistant export ZIP. Preview happens locally and is read-only; nothing is imported or sent to a model.</p>
          {#if ['reviewing', 'extracting'].includes(onboarding.name)}
            <div class="manifest-summary">
              <span>Supported files · {onboarding.selectedNames.length} of {onboarding.manifest.entries.length} selected</span>
              <strong>{onboarding.manifest.entries.length} · {formatByteSize(onboarding.manifest.totalByteSize)} expanded</strong>
            </div>
            {#if onboarding.manifest.entries.length}
              <ol class="manifest" aria-label="Export manifest">
                {#each onboarding.manifest.entries as entry}
                  <li>
                    <label class="manifest-consent"><input type="checkbox" checked={onboarding.selectedNames.includes(entry.name)} onchange={(event) => selectImportEntry(entry.name, event.currentTarget.checked)} disabled={onboarding.name === 'extracting'} /><span class="manifest-meta"><strong>{entry.name}</strong><span>{entry.kind} · {formatByteSize(entry.byteSize)} · {entry.excerptTruncated ? 'excerpt truncated' : 'complete excerpt'}</span></span></label>
                    <pre>{entry.excerpt}</pre>
                  </li>
                {/each}
              </ol>
            {:else}<p class="empty-manifest">No supported files were found in this ZIP.</p>{/if}
          {:else}
            <div class="path-card import-card">
              <span class="path-label">Assistant export</span>
              <strong>{onboarding.archivePath ?? 'No ZIP selected'}</strong>
              <button data-testid="onboarding-import-picker" onclick={chooseImportArchive} disabled={['previewing', 'finalizing'].includes(onboarding.name)}>{onboarding.name === 'previewing' ? 'Reading archive…' : 'Choose ZIP…'}</button>
            </div>
          {/if}
          {#if onboarding.error}<p class="onboarding-error" role="alert">{onboarding.error}</p>{/if}
          <div class="onboarding-footer">
            {#if ['reviewing', 'extracting'].includes(onboarding.name)}<button data-testid="onboarding-import-picker" onclick={chooseImportArchive}>Choose a different ZIP…</button>{:else}<span class="privacy-note">Local preview · no Home writes</span>{/if}
            <div class="onboarding-actions"><button data-testid="onboarding-import-skip" onclick={skipImport} disabled={onboarding.name === 'finalizing'}>{onboarding.name === 'finalizing' ? 'Creating Home…' : 'Continue without importing'}</button>{#if ['reviewing', 'extracting'].includes(onboarding.name)}<button data-testid="onboarding-import-continue" class="primary" onclick={extractImportSelection} disabled={onboarding.name === 'extracting' || onboarding.selectedNames.length === 0}>{onboarding.name === 'extracting' ? 'Reading approved files…' : 'Continue with selected'}</button>{/if}</div>
          </div>
        {:else if ['pre-triage', 'triaging', 'triage-error'].includes(onboarding.name)}
          <p class="support">Generate a local proposal from {onboarding.extractedEntries.length} approved {onboarding.extractedEntries.length === 1 ? 'file' : 'files'}. You will review it before anything can be imported.</p>
          <ul class="triage-sources" aria-label="Approved sources">{#each onboarding.extractedEntries as entry}<li><strong>{entry.sourceName}</strong><span>{entry.sourceProvenance}</span></li>{/each}</ul>
          {#if onboarding.name === 'triaging'}<p class="support" role="status">Generating proposal on this device…</p>{/if}
          {#if onboarding.error}<p class="onboarding-error" role="alert">{onboarding.error}</p>{/if}
          <div class="onboarding-footer">
            <button data-testid="onboarding-triage-back" onclick={returnToArchiveReview}>Back to archive review</button>
            <div class="triage-generate"><button data-testid="onboarding-triage-generate" class="primary" onclick={generateTriageReport} disabled={onboarding.name === 'triaging' || !requiredModel.aiFeaturesAvailable}>{onboarding.name === 'triaging' ? 'Generating proposal…' : onboarding.name === 'triage-error' ? 'Try generating again' : 'Generate local proposal'}</button>{#if !requiredModel.aiFeaturesAvailable}<span>Available when the local AI model is ready.</span>{/if}</div>
          </div>
        {:else if onboarding.name === 'triage-review'}
          <p class="support">Review the local AI proposal and the approved sources that informed it. Confirming only records your choice for this onboarding session.</p>
          <div class="triage-report">
            <section aria-labelledby="triage-user-type"><h2 id="triage-user-type">User type</h2><p>{onboarding.report.userType}</p></section>
            <section aria-labelledby="triage-home-layout"><h2 id="triage-home-layout">Proposed Home layout</h2><p>{onboarding.report.proposedHomeLayout}</p></section>
            <section aria-labelledby="triage-starter-agents"><h2 id="triage-starter-agents">Starter agents</h2><ul>{#each onboarding.report.starterAgents as agent}<li>{agent}</li>{/each}</ul></section>
          </div>
          <h2 class="source-heading">Approved sources</h2>
          <ul class="triage-sources" aria-label="Approved sources">{#each onboarding.extractedEntries as entry}<li><strong>{entry.sourceName}</strong><span>{entry.sourceProvenance}</span></li>{/each}</ul>
          <div class="onboarding-footer"><button data-testid="onboarding-triage-back" onclick={returnToArchiveReview}>Back to archive review</button><button data-testid="onboarding-triage-confirm" class="primary" onclick={() => { onboarding = onboardingTriageConfirmedState(onboarding) }}>Confirm onboarding proposal</button></div>
        {:else if onboarding.name === 'triage-confirmed'}
          <p class="support" role="status">Proposal confirmed for this onboarding session. Nothing has been written to your Muniment Home.</p>
        {:else if onboarding.name === 'load-error'}
          <p class="support">Onboarding could not start.</p><button onclick={loadOnboarding}>Try again</button>
          <p class="onboarding-error" role="alert">{onboarding.error}</p>
        {/if}
      </section>
    {:else if auth.name === 'signed-out'}
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
      <section class="workspace">
        {#if draggingFiles}<div class="drop-affordance" role="status"><strong>Drop files to add them</strong><span>Selected locally · not sent to the model</span></div>{/if}
        <header class="titlebar"><span class="thread-title">New thread</span><span class="thread-id">local · durable</span><span class="title-spacer"></span><button class="quiet" aria-label="Open artifact rail">⌘J</button></header>
        <aside class="sidebar">
          <div class="side-brand"><svg width="24" height="24" viewBox="0 0 48 48" aria-hidden="true"><path d={markD} stroke-width="5" /></svg><strong>muniment</strong></div>
          <button class="side-action">＋ <span>New thread</span><kbd>⌘N</kbd></button>
          <button class="side-action">⌕ <span>Search</span><kbd>⌘F</kbd></button>
          <p class="side-label">Threads</p>
          <button class="thread-row active-thread"><span></span>New thread</button>
          <button class="side-action home-settings" onclick={() => { onboarding = onboardingSettingsState(onboarding) }}>⌂ <span>Home settings</span></button>
          <AccessPanel {tauri} subject={auth.subject} onSignOut={() => run('sign-out')} escapeBlocked={() => dictationRequested || isDictationActive(dictation)} />
        </aside>
        <div class="thread-shell">
        <div class="thread" aria-live="polite" bind:this={thread} onscroll={handleThreadScroll}>
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
                        <li><span>{attachment.displayName}</span><span>{formatByteSize(attachment.byteLength)}</span><strong>Saved locally · not sent to model</strong></li>
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
                <span class="thinking"><svg width="17" height="17" viewBox="0 0 48 48" aria-label="Thinking"><path d={markD} stroke-width="5" /></svg><span>Routing</span></span>
              {:else}<p class:streaming={message.run.phase === 'streaming'}>{message.run.text}{#if message.run.phase === 'streaming'}<span class="caret" aria-hidden="true"></span>{/if}</p>{/if}
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
                {@const parts = receiptParts(message.run.receipt)}
                {@const rows = receiptRows(message.run.receipt)}
                {#if parts.length}
                  {@const expanded = expandedReceipts.has(message.run.id)}
                  <button class="provenance" aria-expanded={expanded} aria-label={`${expanded ? 'Collapse' : 'Expand'} receipt: ${parts.join(', ')}`} onclick={() => toggleReceipt(message.run.id)}><span>{parts[0]}</span>{#if parts.length > 1} · {parts.slice(1).join(' · ')}{/if}</button>
                  {#if expanded}
                    <dl class="receipt-record">
                      {#each rows as row}
                        <div><dt>{row.label}</dt><dd class:route-value={row.route}>{row.value}</dd></div>
                      {/each}
                    </dl>
                  {/if}
                {/if}
              {/if}
            </div>{/if}
          {/each}
        </div>
        {#if !pinned && hasContentBelow}<button class="latest" onclick={scrollToLatest}>↓ latest</button>{/if}
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
            <textarea class:polishing={dictationPolishing} bind:this={composer} bind:value={draft} oninput={composerInput} onkeydown={keydown} rows="2" placeholder={active?.phase === 'resuming' ? 'Resuming interrupted reply…' : 'Ask anything'} disabled={active?.phase === 'resuming'} readonly={dictationPolishing}></textarea>
            {#if dictationPolishing}
              <div class="polish-preview" aria-hidden="true"><span data-testid="polish-draft">{appendTranscript(dictationDraftSnapshot, dictationTranscript).slice(0, -dictationTranscript.length)}</span><span class="polish-transcript" data-testid="polish-transcript">{dictationTranscript}</span></div>
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
          <div class="composer-row">
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
              <button type="button" class="quiet voice" aria-pressed={isDictationActive(dictation)} disabled={!!active || dictationFinishing || dictationPolishing || dictationTransformPending} onpointerdown={voicePointerDown} onpointerup={voicePointerEnd} onpointercancel={voicePointerEnd} onkeydown={voiceKeyDown} onkeyup={voiceKeyUp} onclick={voiceClick}>Voice</button>
              {#if !active}<button type="button" class="quiet attach" onclick={chooseFiles}>Add files</button>{/if}
              {#if active?.phase === 'resuming'}
                <button disabled>Resuming…</button>
              {:else if active && active.id !== 'pending'}
                <button class="quiet follow-up" disabled={!draft.trim()} onclick={() => queue('followUp')}>Queue follow-up</button>
                <button onclick={cancel}>Stop</button>
                <button disabled={!draft.trim()} onclick={() => queue('steer')}>Send</button>
              {:else if !active}<button disabled={!draft.trim() || dictationBusy()} onclick={send}>Send</button>{/if}
            </div>
          </div>
          {#if dictationError}<div class="dictation-error" role="alert">{dictationError}</div>{/if}
          {#if globalVoiceError}<div class="dictation-error" role="alert">The system-wide voice shortcut is unavailable. Voice remains available from the button.</div>{/if}
        </div>
      </section>
    {:else if auth.name === 'error'}
      <section class="auth-state" aria-live="polite">
        <p class="record error-record">{auth.message}</p>
        <button onclick={() => run(auth.retry)}>Try again</button>
      </section>
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
    font-size: 30px;
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

  .onboarding { width: min(680px, calc(100vw - 48px)); margin-top: 28px; }
  .onboarding h1 { margin: 4px 0 10px; font-size: 28px; letter-spacing: -.02em; }
  .model-status { margin: 18px 0 22px; padding: 13px 15px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); }
  .model-status-heading { display: flex; justify-content: space-between; gap: 16px; font: var(--text-12) var(--font-mono); }
  .model-status-heading span, .model-status-copy, .model-progress-copy, .triage-generate span { color: var(--muted); }
  .model-status-copy { margin: 7px 0 0; font-size: 13px; line-height: 1.45; }
  .model-progress { height: 4px; margin-top: 11px; overflow: hidden; border-radius: 2px; background: var(--border); }
  .model-progress span { display: block; height: 100%; background: var(--signal); }
  .model-progress-copy { margin: 6px 0 0; font: var(--text-12) var(--font-mono); }
  .triage-generate { display: flex; flex-direction: column; align-items: flex-end; gap: 6px; }
  .triage-generate span { max-width: 250px; font: var(--text-12) var(--font-mono); text-align: right; }
  .eyebrow, .path-label, .privacy-note { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .path-card { display: grid; grid-template-columns: 1fr auto; gap: 7px 16px; align-items: center; margin-top: 24px; padding: 15px 16px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); }
  .path-label { grid-column: 1 / -1; }
  .path-card strong { min-width: 0; overflow: hidden; text-overflow: ellipsis; font: 13px var(--font-mono); white-space: nowrap; }
  .folder-preview { display: flex; flex-wrap: wrap; gap: 8px; margin: 12px 0 0; }
  .folder-preview span { padding: 4px 8px; border: 1px solid var(--border); border-radius: var(--radius-control); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .onboarding-error { margin: 10px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); line-height: 1.5; }
  .onboarding-footer { display: flex; justify-content: space-between; align-items: center; gap: 16px; margin-top: 22px; }
  .primary { background: var(--ink); border-color: var(--ink); color: var(--paper); }
  .manifest-summary { display: flex; justify-content: space-between; gap: 16px; margin-top: 22px; padding-bottom: 9px; border-bottom: 1px solid var(--border); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .manifest-summary strong { color: var(--ink); font-weight: 500; }
  .manifest { max-height: min(42vh, 360px); margin: 0; padding: 0; overflow-y: auto; list-style: none; }
  .manifest li { padding: 12px 0; border-bottom: 1px solid var(--border); }
  .manifest-consent { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 10px; align-items: start; cursor: pointer; }
  .manifest-consent input { margin-top: 2px; accent-color: var(--ink); }
  .manifest-meta { display: flex; justify-content: space-between; gap: 16px; font: var(--text-12) var(--font-mono); }
  .manifest-meta strong { min-width: 0; overflow-wrap: anywhere; font-weight: 500; }
  .manifest-meta span { flex: 0 0 auto; color: var(--muted); }
  .manifest pre { max-height: 96px; margin: 8px 0 0; padding: 8px 10px; overflow: auto; border-radius: var(--radius-control); background: var(--faint); color: var(--muted); font: var(--text-12) var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  .empty-manifest { margin: 0; padding: 18px 0; border-bottom: 1px solid var(--border); color: var(--muted); font: var(--text-12) var(--font-mono); }
  .onboarding-actions { display: flex; gap: 8px; }
  .triage-report { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px; margin-top: 22px; }
  .triage-report section { min-width: 0; padding: 14px; border: 1px solid var(--border); border-radius: 6px; background: var(--surface); }
  .triage-report h2, .source-heading { margin: 0 0 8px; font-size: var(--text-13); }
  .triage-report p, .triage-report ul { margin: 0; padding-left: 18px; line-height: var(--leading-body); white-space: pre-wrap; overflow-wrap: anywhere; }
  .triage-report p { padding-left: 0; }
  .source-heading { margin-top: 20px; }
  .triage-sources { margin: 0; padding: 0; border-top: 1px solid var(--border); list-style: none; }
  .triage-sources li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 12px; padding: 8px 0; border-bottom: 1px solid var(--border); font: var(--text-12) var(--font-mono); }
  .triage-sources strong { overflow-wrap: anywhere; font-weight: 500; }
  .triage-sources span { color: var(--muted); overflow-wrap: anywhere; }

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

  button:focus-visible {
    outline: 2px solid var(--signal);
    outline-offset: 1px;
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
  .workspace { grid-template-columns: 260px 1fr; grid-template-areas: "title title" "side thread" "side composer"; }
  .drop-affordance { position: fixed; z-index: 4; inset: 52px 0 0 260px; display: grid; place-content: center; gap: 5px; background: color-mix(in srgb, var(--paper) 92%, transparent); border: 1px dashed var(--muted); color: var(--ink); text-align: center; pointer-events: none; }
  .drop-affordance span { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .titlebar { grid-area: title; display: flex; align-items: center; padding: 0 18px 0 278px; border-bottom: 1px solid var(--border); background: var(--surface); }
  .thread-title { font-weight: 600; }
  .thread-id, kbd { margin-left: 10px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .title-spacer { flex: 1; }
  .sidebar { grid-area: side; min-width: 0; display: flex; flex-direction: column; padding: 14px 10px 10px; background: var(--surface); border-right: 1px solid var(--border); }
  .side-brand { display: flex; align-items: center; gap: 10px; padding: 2px 8px 16px; }
  .side-brand path { fill: none; stroke: var(--ink); stroke-linecap: round; }
  .side-action, .thread-row { width: 100%; display: flex; align-items: center; gap: 9px; padding: 7px 8px; border-color: transparent; background: transparent; text-align: left; }
  .side-action span { flex: 1; }
  .side-label { margin: 20px 8px 5px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .active-thread { background: var(--faint); }
  .active-thread > span { width: 5px; height: 5px; border-radius: 50%; background: var(--signal); }
  .quiet { background: transparent; border-color: transparent; }
  .thread-shell { grid-area: thread; position: relative; min-height: 0; }
  .thread { width: min(760px, calc(100% - 48px)); height: 100%; margin: 0 auto; padding: 42px 0; overflow-y: auto; }
  .latest { position: absolute; left: 50%; bottom: 14px; transform: translateX(-50%); border-radius: 6px; background: var(--surface); color: var(--muted); font: var(--text-12) var(--font-mono); box-shadow: 0 1px 3px color-mix(in srgb, var(--ink) 10%, transparent); }
  .empty { color: var(--muted); text-align: center; margin-top: 18vh; }
  .user-turn { max-width: 78%; margin: 0 0 28px auto; }
  .user-message { width: fit-content; margin-left: auto; padding: 9px 13px; background: var(--faint); border-radius: 10px; }
  .user-message > p { margin: 0; white-space: pre-wrap; }
  .missing-prompt { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .message-attachments { display: grid; justify-items: end; gap: 4px; margin: 8px 0 0; padding: 0; list-style: none; }
  .message-attachments li { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 5px 8px; max-width: 100%; padding: 5px 8px; border: 1px solid var(--border); border-radius: 2px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .message-attachments strong { flex-basis: 100%; color: var(--muted); font-weight: 400; font-size: 10px; }
  .response { margin: 0 0 34px; }
  .response p { white-space: pre-wrap; }
  .streaming { display: inline; border-bottom: 2px solid var(--signal); }
  .caret { display: inline-block; height: 1em; border-right: 2px solid var(--signal); margin-left: 2px; vertical-align: -2px; animation: blink 800ms step-end infinite; }
  .thinking { display: flex; align-items: center; gap: 9px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .thinking path { fill: none; stroke: var(--signal); stroke-linecap: round; animation: breathe 1.8s ease-in-out infinite; }
  .tool-card { margin-top: 8px; padding: 8px 12px; border: 1px solid var(--border); border-radius: var(--radius-control); background: var(--surface); color: var(--muted); font: 12.5px var(--font-mono); }
  .tool-row { display: flex; align-items: center; gap: 8px; min-height: 20px; }
  .tool-group-title { margin-bottom: 4px; color: var(--muted); }
  .tool-group .tool-row + .tool-row { margin-top: 4px; }
  .tool-dot { width: 7px; height: 7px; flex: 0 0 auto; border-radius: 50%; background: currentColor; }
  .tool-name { min-width: 0; overflow-wrap: anywhere; }
  .tool-status { margin-left: auto; }
  .tool-running { color: var(--signal); }
  .tool-running .tool-dot { animation: tool-pulse 1.4s ease-in-out infinite; }
  .tool-failed .tool-status::before { content: 'error · '; }
  .provenance { display: block; margin-top: 10px; padding: 0; border: 0; background: transparent; color: var(--muted); font: var(--text-12) var(--font-mono); text-align: left; }
  .provenance span { color: var(--signal); }
  .receipt-record { width: fit-content; min-width: 240px; margin: 8px 0 0; padding: 8px 12px; border: 1px solid var(--border); border-radius: 6px; color: var(--muted); font-size: var(--text-12); }
  .receipt-record div { display: grid; grid-template-columns: 88px minmax(0, 1fr); gap: 12px; }
  .receipt-record dd { margin: 0; font-family: var(--font-mono); font-variant-numeric: tabular-nums; overflow-wrap: anywhere; }
  .receipt-record .route-value { color: var(--signal); }
  .run-error { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .cancel-error, .history-error { margin: 0 0 8px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .run-error button { padding: 2px 6px; }
  .composer { grid-area: composer; width: min(760px, calc(100% - 48px)); margin: 0 auto 24px; padding: 12px; background: var(--surface); border: 1px solid var(--border); border-radius: 10px; }
  .composer:focus-within { border-color: var(--muted); }
  .attachments { display: flex; flex-wrap: wrap; gap: 6px; margin: 0 0 8px; padding: 0; list-style: none; }
  .attachments li { display: flex; align-items: center; gap: 6px; max-width: 100%; padding: 4px 6px 4px 9px; border: 1px solid var(--border); border-radius: 2px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .attachments span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .attachments button { padding: 1px 5px; border: 0; background: transparent; color: inherit; font-size: 11px; }
  .composer-input { position: relative; }
  textarea { display: block; width: 100%; resize: none; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; }
  textarea.polishing { color: transparent; caret-color: transparent; }
  .polish-preview { position: absolute; inset: 0; overflow: hidden; pointer-events: none; white-space: pre-wrap; color: var(--ink); font: inherit; }
  .polish-transcript { text-decoration-line: underline; text-decoration-color: var(--signal); text-decoration-thickness: 2px; text-underline-offset: 3px; }
  .dictation-transforms { display: flex; flex-wrap: wrap; gap: 5px; margin: 7px 0; }
  .dictation-transforms button { display: inline-flex; align-items: center; gap: 7px; padding: 3px 7px; border-color: var(--signal); border-radius: 2px; background: transparent; color: var(--signal); font: var(--text-12) var(--font-mono); }
  .dictation-transforms button:hover:not(:disabled) { background: var(--signal-soft); }
  .dictation-transforms button:focus-visible { outline-color: var(--ink); outline-offset: 2px; }
  .dictation-transforms button:disabled { border-color: var(--border); color: var(--muted); }
  .dictation-transforms kbd { color: var(--muted); font: inherit; }
  .composer-row { display: flex; justify-content: space-between; align-items: center; color: var(--muted); font-size: 11px; }
  .composer-actions { display: flex; align-items: center; gap: 6px; }
  .capture-status { display: flex; align-items: center; gap: 8px; font-family: var(--font-mono); }
  .capture-meter { height: 14px; display: flex; align-items: center; gap: 2px; }
  .capture-meter i { width: 2px; height: 6px; background: var(--muted); animation: capture 900ms ease-in-out infinite alternate; }
  .capture-meter i:nth-child(2), .capture-meter i:nth-child(4) { height: 10px; animation-delay: -300ms; }
  .capture-meter i:nth-child(3) { height: 14px; animation-delay: -600ms; }
  .polish-status { color: var(--signal); font-family: var(--font-mono); text-decoration: underline 2px; text-underline-offset: 3px; }
  .dictation-error { margin-top: 7px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .follow-up { color: var(--muted); font-family: var(--font-mono); }
  @keyframes blink { 50% { opacity: 0; } }
  @keyframes breathe { 50% { opacity: .45; } }
  @keyframes tool-pulse { 50% { opacity: .3; transform: scale(.75); } }
  @keyframes capture { to { transform: scaleY(.55); } }
  @media (prefers-reduced-motion: reduce) { .caret, .thinking path, .tool-running .tool-dot, .capture-meter i { animation: none; } }
</style>
