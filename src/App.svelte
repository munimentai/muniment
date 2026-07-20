<script>
  import { onMount, tick } from 'svelte'
  import { getCurrentWebview } from '@tauri-apps/api/webview'
  import { open } from '@tauri-apps/plugin-dialog'

  import AccessPanel from './lib/AccessPanel.svelte'
  import { bootState, errorState, statusState, waitingState } from './lib/auth-state.js'
  import { ringPath } from './lib/mark.js'
  import { applyBufferedChatEvents, applyChatEvent, composerAction, formatByteSize, historyMessages, receiptParts, receiptRows, toolName, toolStatus } from './lib/chat-state.js'
  import { appendTranscript, isDictationActive } from './lib/dictation-state.js'
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
  let dictationDraftSnapshot = ''
  let suppressVoiceClick = false
  let voiceClickTimer
  let voicePointerId
  let voiceKey
  let composer = $state()
  const defaultChoices = () => ({ layout: ['memory', 'agents', 'projects', 'sessions'], starterAgents: ['researcher', 'writer'] })
  let onboarding = $state({ state: 'loading', homePath: '', warning: null, locationConfirmed: false, choices: defaultChoices() })
  let onboardingError = $state('')
  let acquisition = $state(null)
  let lateTriageAvailable = $state(false)
  let lateTriageRetrying = $state(false)
  let acquisitionTimer
  let destroyed = false

  async function loadOnboarding() {
    try {
      const status = await tauri.invoke('onboarding_status')
      onboarding = status.complete
        ? { state: 'complete', choices: defaultChoices(), ...status }
        : { state: 'location', locationConfirmed: false, choices: defaultChoices(), ...status }
      if (!status.complete || status.triagePending) pollAcquisition()
    } catch (error) {
      onboardingError = typeof error === 'string' ? error : 'Onboarding could not be loaded.'
      onboarding = { state: 'error', homePath: '', warning: null }
    }
  }

  async function pollAcquisition() {
    clearTimeout(acquisitionTimer)
    if (destroyed || (onboarding.state === 'complete' && !onboarding.triagePending)) return
    try { acquisition = await tauri.invoke('required_model_acquisition_status') } catch (_) { acquisition = null }
    if (onboarding.state === 'complete' && onboarding.triagePending && acquisition?.aiFeaturesAvailable) lateTriageAvailable = true
    if (!destroyed && (onboarding.state !== 'complete' || (onboarding.triagePending && !lateTriageAvailable))) acquisitionTimer = setTimeout(pollAcquisition, 1000)
  }

  async function chooseHome() {
    onboardingError = ''
    try {
      const picked = await open({ directory: true, multiple: false, defaultPath: onboarding.homePath })
      if (picked) onboarding = { ...onboarding, homePath: picked, warning: null, locationConfirmed: true }
    } catch (_) { onboardingError = 'The folder picker could not be opened. Try again.' }
  }

  function acceptLocation() {
    onboardingError = ''
    onboarding = { ...onboarding, locationConfirmed: true }
  }

  function toggleChoice(group, value) {
    const selected = onboarding.choices[group]
    onboarding = { ...onboarding, choices: { ...onboarding.choices, [group]: selected.includes(value) ? selected.filter((item) => item !== value) : [...selected, value] } }
  }

  async function proposeOnboarding(requireModel = false) {
    if (!onboarding.locationConfirmed && !requireModel) return
    onboardingError = ''
    const previousState = onboarding.state
    onboarding = { ...onboarding, state: 'proposing' }
    try {
      const proposal = await tauri.invoke('onboarding_propose', { homePath: onboarding.homePath, choices: onboarding.choices, requireModel })
      onboarding = { ...onboarding, ...proposal, state: 'report', lateTriage: requireModel }
    } catch (error) {
      onboarding = { ...onboarding, state: requireModel ? 'complete' : previousState }
      onboardingError = typeof error === 'string' ? error : 'The onboarding report could not be created.'
      if (requireModel) {
        lateTriageRetrying = true
        setTimeout(() => { lateTriageRetrying = false }, 5000)
      }
    }
  }

  async function confirmOnboarding() {
    onboardingError = ''
    onboarding = { ...onboarding, state: 'scaffolding' }
    try {
      const status = await tauri.invoke('onboarding_confirm', { homePath: onboarding.homePath, choices: onboarding.choices, lateTriage: !!onboarding.lateTriage })
      onboarding = { state: 'complete', choices: onboarding.choices, ...status }
      lateTriageAvailable = false
      clearTimeout(acquisitionTimer)
      if (status.triagePending) pollAcquisition()
    } catch (error) {
      onboarding = { ...onboarding, state: 'report' }
      onboardingError = typeof error === 'string' ? error : 'Muniment Home could not be created.'
    }
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
    return dictationCommandPending || isDictationActive(dictation)
  }

  function applyDictationStatus(status) {
    dictation = status
    if (status.state === 'modelNotInstalled' || status.state === 'failed') {
      dictationError = status.message
    } else dictationError = ''
    if (!isDictationActive(status)) stopDictationPolling()
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
    dictationRequested = false
    invalidateDictationPolls()
    if (cancelled) {
      dictationCancelled = true
      voicePointerId = undefined
      voiceKey = undefined
      draft = dictationDraftSnapshot
      tick().then(() => composer?.focus())
    }
    if (dictationCommandPending || !isDictationActive(dictation)) return
    dictationCommandPending = true
    dictationError = ''
    try {
      const status = await tauri.invoke('dictation_stop')
      if (destroyed) return
      applyDictationStatus(status)
      if (isDictationActive(dictation)) pollDictation()
    } catch (error) {
      if (destroyed) return
      dictationError = typeof error === 'string' ? error : 'Dictation could not be stopped.'
      pollDictation()
    } finally {
      dictationCommandPending = false
    }
  }

  async function startDictation() {
    if (active || dictationCommandPending || dictationRequested) return
    if (isDictationActive(dictation)) {
      await stopDictation()
      return
    }
    invalidateDictationPolls()
    dictationRequested = true
    dictationCancelled = false
    dictationDraftSnapshot = draft
    dictationCommandPending = true
    dictationError = ''
    try {
      const status = await tauri.invoke('dictation_start')
      if (destroyed) return
      applyDictationStatus(status)
      if (!dictationRequested && isDictationActive(dictation)) {
        dictationCommandPending = false
        await stopDictation()
      } else if (isDictationActive(dictation)) pollDictation()
    } catch (error) {
      if (destroyed) return
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

  function voicePointerDown(event) {
    if (event.button !== 0 || voicePointerId !== undefined || voiceKey !== undefined) return
    event.preventDefault()
    expectVoiceClick()
    voicePointerId = event.pointerId
    event.currentTarget.setPointerCapture?.(event.pointerId)
    startDictation()
  }

  function voicePointerEnd(event) {
    if (event.pointerId !== voicePointerId) return
    event.preventDefault()
    expectVoiceClick()
    voicePointerId = undefined
    stopDictation(event.type === 'pointercancel')
  }

  function voiceKeyDown(event) {
    if (event.key !== ' ' && event.key !== 'Enter') return
    event.preventDefault()
    expectVoiceClick()
    if (!event.repeat) {
      if (voiceKey !== undefined || voicePointerId !== undefined) return
      voiceKey = event.key
      startDictation()
    }
  }

  function voiceKeyUp(event) {
    if (event.key !== voiceKey) return
    event.preventDefault()
    expectVoiceClick()
    voiceKey = undefined
    stopDictation()
  }

  function voiceClick() {
    if (suppressVoiceClick) {
      suppressVoiceClick = false
      clearTimeout(voiceClickTimer)
      return
    }
    dictationRequested || isDictationActive(dictation) ? stopDictation() : startDictation()
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
    if (tauri) {
      loadOnboarding()
      run('status')
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
    window.__TAURI__?.event?.listen('dictation-event', ({ payload }) => {
      if (payload.type === 'transcript' && !dictationCancelled) draft = appendTranscript(draft, payload.text)
    }).then((stop) => {
      if (destroyed) stop()
      else dictationUnlisten = stop
    })
    const escape = (event) => {
      if (event.key === 'Escape' && (dictationRequested || isDictationActive(dictation))) {
        event.preventDefault()
        stopDictation(true)
        return
      }
    }
    document.addEventListener('keydown', escape)
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
      unlisten?.()
      dictationUnlisten?.()
      stopDictationPolling()
      clearTimeout(acquisitionTimer)
      clearTimeout(voiceClickTimer)
      stopDragDrop?.()
      document.removeEventListener('keydown', escape)
    }
  })

  async function send() {
    const prompt = draft.trim()
    if (!prompt || active || dictationBusy()) return
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
    {#if onboarding.state !== 'complete'}
      <section class="onboarding" aria-labelledby="onboarding-title">
        <p class="eyebrow">First-run setup</p>
        <h1 id="onboarding-title">Choose your Muniment Home</h1>
        {#if onboarding.state === 'loading'}
          <p class="support" role="status">Finding your Documents folder…</p>
        {:else if onboarding.state === 'location' || onboarding.state === 'proposing'}
          <p class="support">Your memory stays in plain Markdown files in a folder you control. Confirm a location to continue.</p>
          <div class="path-card"><label for="onboarding-home-path">Home location</label><input id="onboarding-home-path" data-testid="onboarding-home-path" bind:value={onboarding.homePath} oninput={() => { onboarding.locationConfirmed = false }} disabled={onboarding.state === 'proposing'} /><button data-testid="onboarding-picker" onclick={chooseHome} disabled={onboarding.state === 'proposing'}>Choose folder…</button></div>
          {#if onboarding.warning}<p class="location-warning" role="status">{onboarding.warning}</p>{/if}
          <button data-testid="onboarding-accept-location" class="quiet" onclick={acceptLocation} disabled={onboarding.state === 'proposing' || onboarding.locationConfirmed}>{onboarding.locationConfirmed ? 'Location accepted' : 'Use this location'}</button>
          <div class="onboarding-footer">
            <span class="model-progress">{acquisition?.aiFeaturesAvailable ? 'On-device model ready' : acquisition?.status?.state === 'installing' ? `Model downloading in background · ${acquisition.totalBytes ? Math.round((acquisition.downloadedBytes / acquisition.totalBytes) * 100) : 0}%` : 'Manual setup available · model triage will run later'}</span>
            <button data-testid="onboarding-review" class="primary" onclick={() => proposeOnboarding(false)} disabled={onboarding.state === 'proposing' || !onboarding.locationConfirmed}>{onboarding.state === 'proposing' ? 'Preparing…' : 'Review setup'}</button>
          </div>
        {:else if onboarding.state === 'revise'}
          <p class="support">Choose what the revised report and Home should contain, or request another model proposal.</p>
          <fieldset data-testid="onboarding-layout"><legend>Home layout</legend>{#each ['memory', 'agents', 'projects', 'sessions'] as item}<label><input type="checkbox" checked={onboarding.choices.layout.includes(item)} onchange={() => toggleChoice('layout', item)} /> {item}/</label>{/each}</fieldset>
          <fieldset data-testid="onboarding-agents"><legend>Starter agents</legend>{#each ['researcher', 'writer'] as item}<label><input type="checkbox" checked={onboarding.choices.starterAgents.includes(item)} onchange={() => toggleChoice('starterAgents', item)} /> {item}</label>{/each}</fieldset>
          <div class="onboarding-footer"><button class="quiet" onclick={() => { onboarding = { ...onboarding, state: 'location' } }}>Change location</button><button data-testid="onboarding-repropose" class="primary" onclick={() => proposeOnboarding(false)} disabled={onboarding.choices.layout.length === 0}>Generate revised report</button></div>
        {:else if onboarding.state === 'report' || onboarding.state === 'scaffolding'}
          <p class="support">Review this report before Muniment creates any folders or starter agents.</p>
          <div class="report-mode">{onboarding.usedModel ? 'Prepared by the on-device model' : 'Manual fallback · model triage will be offered when ready'}</div>
          {#if onboarding.warning}<p class="location-warning" role="status">{onboarding.warning}</p>{/if}
          <pre data-testid="onboarding-report" class="onboarding-report">{onboarding.report}</pre>
          <div class="onboarding-footer"><button data-testid="onboarding-reject" class="quiet" onclick={() => { onboarding = { ...onboarding, state: 'revise' } }} disabled={onboarding.state === 'scaffolding'}>Reject and revise</button><button data-testid="onboarding-confirm" class="primary" onclick={confirmOnboarding} disabled={onboarding.state === 'scaffolding'}>{onboarding.state === 'scaffolding' ? 'Creating Home…' : onboarding.lateTriage ? 'Confirm triage report' : 'Confirm and create Home'}</button></div>
        {:else if onboarding.state === 'error'}
          <p class="support">Onboarding could not start.</p><button onclick={loadOnboarding}>Try again</button>
        {/if}
        {#if onboardingError}<p class="onboarding-error" role="alert">{onboardingError}</p>{/if}
      </section>
    {:else if auth.name === 'signed-out'}
      <section class="auth-state">
        {#if onboarding.triagePending}<div class="late-triage"><span>{lateTriageAvailable ? 'On-device triage is ready when you are.' : 'On-device triage is pending.'}</span>{#if lateTriageAvailable}<button data-testid="onboarding-late-triage" onclick={() => proposeOnboarding(true)} disabled={lateTriageRetrying}>{lateTriageRetrying ? 'Retry shortly…' : 'Review triage'}</button>{/if}</div>{/if}
        {#if onboardingError}<p class="onboarding-error" role="alert">{onboardingError}</p>{/if}
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
        {#if onboarding.triagePending}<div class="late-triage"><span>{lateTriageAvailable ? 'On-device triage is ready when you are.' : 'On-device triage is pending.'}</span>{#if lateTriageAvailable}<button data-testid="onboarding-late-triage" onclick={() => proposeOnboarding(true)} disabled={lateTriageRetrying}>{lateTriageRetrying ? 'Retry shortly…' : 'Review triage'}</button>{/if}</div>{/if}
        {#if onboardingError}<p class="onboarding-error late-triage-error" role="alert">{onboardingError}</p>{/if}
        {#if draggingFiles}<div class="drop-affordance" role="status"><strong>Drop files to add them</strong><span>Selected locally · not sent to the model</span></div>{/if}
        <header class="titlebar"><span class="thread-title">New thread</span><span class="thread-id">local · durable</span><span class="title-spacer"></span><button class="quiet" aria-label="Open artifact rail">⌘J</button></header>
        <aside class="sidebar">
          <div class="side-brand"><svg width="24" height="24" viewBox="0 0 48 48" aria-hidden="true"><path d={markD} stroke-width="5" /></svg><strong>muniment</strong></div>
          <button class="side-action">＋ <span>New thread</span><kbd>⌘N</kbd></button>
          <button class="side-action">⌕ <span>Search</span><kbd>⌘F</kbd></button>
          <p class="side-label">Threads</p>
          <button class="thread-row active-thread"><span></span>New thread</button>
          <button class="side-action home-settings" onclick={() => { onboarding = { ...onboarding, state: 'location', locationConfirmed: false, choices: onboarding.choices ?? defaultChoices() }; pollAcquisition() }}>⌂ <span>Home settings</span></button>
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
          <textarea bind:this={composer} bind:value={draft} onkeydown={keydown} rows="2" placeholder={active?.phase === 'resuming' ? 'Resuming interrupted reply…' : 'Ask anything'} disabled={active?.phase === 'resuming'}></textarea>
          {#if submitError}<p class="cancel-error" role="alert">{submitError}</p>{/if}
          {#if cancelError}<p class="cancel-error" role="alert">{cancelError}</p>{/if}
          {#if queueError}<p class="cancel-error" role="alert">{queueError}</p>{/if}
          <div class="composer-row">
            {#if isDictationActive(dictation)}
              <span class="capture-status" role="status">
                <span class="capture-meter" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i></span>
                {dictation.state === 'starting' ? 'Starting local dictation…' : 'Listening on this device…'}
              </span>
            {:else}
              <span>{active?.phase === 'resuming' ? 'Reopening the existing secure session…' : active && active.id !== 'pending' ? '⏎ steers this reply · queue as follow-up' : 'Routing is automatic. Every reply carries its receipt.'}</span>
            {/if}
            <div class="composer-actions">
              <button type="button" class="quiet voice" aria-pressed={isDictationActive(dictation)} disabled={!!active} onpointerdown={voicePointerDown} onpointerup={voicePointerEnd} onpointercancel={voicePointerEnd} onkeydown={voiceKeyDown} onkeyup={voiceKeyUp} onclick={voiceClick}>Voice</button>
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
  .eyebrow, .path-card label, .model-progress, .report-mode { color: var(--muted); font: var(--text-12) var(--font-mono); }
  .path-card { display: grid; grid-template-columns: 1fr auto; gap: 7px 16px; align-items: center; margin-top: 24px; padding: 15px 16px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); }
  .path-card label { grid-column: 1 / -1; }
  .path-card input { min-width: 0; width: 100%; border: 0; background: transparent; color: var(--ink); font: 13px var(--font-mono); }
  .path-card input:focus { outline: 2px solid var(--focus); outline-offset: 3px; }
  .location-warning, .onboarding-error { margin: 10px 0 0; color: var(--muted); font: var(--text-12) var(--font-mono); line-height: 1.5; }
  .location-warning { padding-left: 10px; border-left: 2px solid var(--signal); }
  .onboarding-footer { display: flex; justify-content: space-between; align-items: center; gap: 16px; margin-top: 22px; }
  .primary { background: var(--ink); border-color: var(--ink); color: var(--paper); }
  .report-mode { margin: 20px 0 8px; }
  .onboarding-report { max-height: 330px; margin: 0; padding: 18px; overflow: auto; white-space: pre-wrap; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); color: var(--ink); font: 12px/1.55 var(--font-mono); }
  .onboarding fieldset { display: grid; gap: 8px; margin: 16px 0; padding: 14px 16px; border: 1px solid var(--border); border-radius: 8px; }
  .onboarding fieldset label { font: var(--text-13) var(--font-mono); }
  .late-triage { display: flex; align-items: center; justify-content: space-between; gap: 16px; padding: 10px 14px; border: 1px solid var(--border); border-radius: 8px; background: var(--surface); font: var(--text-12) var(--font-mono); }

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
  textarea { width: 100%; resize: none; border: 0; outline: 0; background: transparent; color: var(--ink); font: inherit; }
  .composer-row { display: flex; justify-content: space-between; align-items: center; color: var(--muted); font-size: 11px; }
  .composer-actions { display: flex; align-items: center; gap: 6px; }
  .capture-status { display: flex; align-items: center; gap: 8px; font-family: var(--font-mono); }
  .capture-meter { height: 14px; display: flex; align-items: center; gap: 2px; }
  .capture-meter i { width: 2px; height: 6px; background: var(--muted); animation: capture 900ms ease-in-out infinite alternate; }
  .capture-meter i:nth-child(2), .capture-meter i:nth-child(4) { height: 10px; animation-delay: -300ms; }
  .capture-meter i:nth-child(3) { height: 14px; animation-delay: -600ms; }
  .dictation-error { margin-top: 7px; color: var(--muted); font: var(--text-12) var(--font-mono); }
  .follow-up { color: var(--muted); font-family: var(--font-mono); }
  @keyframes blink { 50% { opacity: 0; } }
  @keyframes breathe { 50% { opacity: .45; } }
  @keyframes tool-pulse { 50% { opacity: .3; transform: scale(.75); } }
  @keyframes capture { to { transform: scaleY(.55); } }
  @media (prefers-reduced-motion: reduce) { .caret, .thinking path, .tool-running .tool-dot, .capture-meter i { animation: none; } }
</style>
