import { appendTranscript, isDictationActive } from './dictation-state.js'

const transcriptQuietPeriod = 25
const transformOfferPeriod = 6000

export function createDictationController({
  invoke,
  listen,
  readDraft,
  updateDraft,
  blocked = () => false,
  onState,
  onError,
  onInactive = () => {},
  onCancel = () => {},
  onFocus = () => {},
}) {
  let status = { state: 'idle' }
  let pollTimer
  let pollEpoch = 0
  let unlisten
  let commandPending = false
  let requested = false
  let cancelled = true
  let draftSnapshot = ''
  let transcript = ''
  let captureEpoch = 0
  let completionEpoch
  let completionTimer
  let finishing = false
  let polishEpoch
  let polishing = false
  let transformEpoch = 0
  let transformPending = false
  let transformPendingEpoch
  let eligible = null
  let eligibleTimer
  let eligibleTimerEpoch = 0
  let destroyed = false

  function snapshot() {
    return {
      status,
      commandPending,
      requested,
      finishing,
      polishing,
      transformPending,
      eligible,
      draftSnapshot,
      transcript,
    }
  }

  function publish() {
    if (!destroyed) onState(snapshot())
  }

  function error(message) {
    if (!destroyed) onError(message)
  }

  function busy() {
    return commandPending || finishing || polishing || transformPending || isDictationActive(status)
  }

  function stopPolling() {
    clearTimeout(pollTimer)
    pollTimer = undefined
  }

  function invalidatePolls() {
    pollEpoch += 1
    stopPolling()
  }

  function invalidateTransform() {
    transformEpoch += 1
    eligibleTimerEpoch += 1
    clearTimeout(eligibleTimer)
    eligibleTimer = undefined
    eligible = null
    publish()
  }

  function offerTransforms(next) {
    const timerEpoch = ++eligibleTimerEpoch
    clearTimeout(eligibleTimer)
    eligible = next
    publish()
    eligibleTimer = setTimeout(() => {
      if (timerEpoch === eligibleTimerEpoch) invalidateTransform()
    }, transformOfferPeriod)
  }

  async function listenForDictation(epoch) {
    unlisten?.()
    unlisten = undefined
    const stop = await listen('dictation-event', ({ payload }) => {
      if (epoch !== captureEpoch || payload.type !== 'transcript' || cancelled || (!isDictationActive(status) && !finishing)) return
      transcript = appendTranscript(transcript, payload.text)
      updateDraft(appendTranscript(draftSnapshot, transcript))
      if (finishing) waitForTranscriptQuiet(epoch)
    })
    if (!stop) return
    if (destroyed || epoch !== captureEpoch) stop()
    else unlisten = stop
  }

  function complete(epoch) {
    if (destroyed || epoch !== captureEpoch || epoch !== completionEpoch) return
    completionEpoch = undefined
    finishing = false
    publish()
    if (!cancelled) {
      polishEpoch = epoch
      void polish(epoch)
    }
  }

  function waitForTranscriptQuiet(epoch) {
    clearTimeout(completionTimer)
    completionTimer = setTimeout(() => complete(epoch), transcriptQuietPeriod)
  }

  function finish(epoch) {
    clearTimeout(completionTimer)
    completionTimer = setTimeout(() => waitForTranscriptQuiet(epoch))
  }

  async function polish(epoch) {
    if (cancelled || epoch !== captureEpoch || epoch !== polishEpoch) return
    const captured = transcript
    if (!captured.trim()) {
      polishEpoch = undefined
      return
    }
    polishing = true
    error('')
    publish()
    const verbatimDraft = appendTranscript(draftSnapshot, captured)
    try {
      const polished = await invoke('dictation_polish', { transcript: captured })
      if (destroyed || cancelled || epoch !== captureEpoch || epoch !== polishEpoch) return
      if (readDraft() === verbatimDraft && polished.trim()) {
        const draft = appendTranscript(draftSnapshot, polished)
        updateDraft(draft)
        offerTransforms({ epoch, snapshot: draftSnapshot, segment: polished, draft })
      }
    } catch (_) {
      if (destroyed || cancelled || epoch !== captureEpoch || epoch !== polishEpoch) return
      error('Polishing is unavailable. You can edit or send the captured text.')
    } finally {
      if (epoch === polishEpoch) {
        polishEpoch = undefined
        polishing = false
        publish()
      }
    }
  }

  function applyStatus(next) {
    status = next
    if (!isDictationActive(next)) {
      onInactive()
      requested = false
      stopPolling()
    }
    error(next.state === 'modelNotInstalled' || next.state === 'failed' ? next.message : '')
    if (next.state === 'stopped' && completionEpoch !== undefined) {
      if (cancelled) complete(completionEpoch)
      else {
        finishing = true
        finish(completionEpoch)
      }
    }
    publish()
  }

  function poll() {
    stopPolling()
    const epoch = pollEpoch
    pollTimer = setTimeout(async () => {
      if (destroyed || epoch !== pollEpoch || !isDictationActive(status)) return
      try {
        const next = await invoke('dictation_status')
        if (destroyed || epoch !== pollEpoch) return
        applyStatus(next)
      } catch (reason) {
        if (destroyed || epoch !== pollEpoch) return
        error(typeof reason === 'string' ? reason : 'Dictation status could not be checked.')
      }
      if (!destroyed && epoch === pollEpoch && isDictationActive(status)) poll()
    }, 100)
  }

  async function stop(cancel = false) {
    onInactive()
    requested = false
    invalidatePolls()
    if (cancel) {
      cancelled = true
      polishEpoch = undefined
      polishing = false
      updateDraft(draftSnapshot)
      onCancel()
      Promise.resolve().then(onFocus)
      publish()
    }
    if (commandPending || !isDictationActive(status)) return
    completionEpoch = captureEpoch
    finishing = true
    commandPending = true
    error('')
    publish()
    try {
      const next = await invoke('dictation_stop')
      if (destroyed) return
      applyStatus(next)
      if (isDictationActive(status)) poll()
    } catch (reason) {
      if (destroyed) return
      finishing = false
      error(typeof reason === 'string' ? reason : 'Dictation could not be stopped.')
      poll()
    } finally {
      commandPending = false
      publish()
    }
  }

  async function start() {
    if (blocked() || busy() || requested) return
    if (isDictationActive(status)) {
      await stop()
      return
    }
    invalidatePolls()
    requested = true
    invalidateTransform()
    cancelled = false
    draftSnapshot = readDraft()
    transcript = ''
    captureEpoch += 1
    completionEpoch = undefined
    finishing = false
    polishEpoch = undefined
    commandPending = true
    error('')
    status = { state: 'starting' }
    publish()
    try {
      await listenForDictation(captureEpoch)
      if (destroyed || cancelled) return
      const next = await invoke('dictation_start')
      if (destroyed) return
      applyStatus(next)
      if (!requested && isDictationActive(status)) {
        commandPending = false
        await stop()
      } else if (isDictationActive(status)) poll()
    } catch (reason) {
      if (destroyed) return
      onInactive()
      status = { state: 'failed' }
      error(typeof reason === 'string' ? reason : 'Dictation could not be started.')
      requested = false
    } finally {
      commandPending = false
      publish()
    }
  }

  async function transform(action) {
    const current = eligible
    if (!current || busy() || readDraft() !== current.draft) return
    const operation = ++transformEpoch
    clearTimeout(eligibleTimer)
    eligibleTimer = undefined
    transformPending = true
    transformPendingEpoch = operation
    error('')
    publish()
    try {
      const transformed = await invoke('dictation_transform', { transform: action.transform, transcript: current.segment })
      if (destroyed || operation !== transformEpoch || readDraft() !== current.draft) return
      if (!transformed.trim()) throw new Error('empty transform')
      updateDraft(appendTranscript(current.snapshot, transformed))
      offerTransforms({ ...current, segment: transformed, draft: readDraft() })
    } catch (_) {
      if (destroyed || operation !== transformEpoch) return
      error('That voice transform is unavailable. Your text is unchanged; try again.')
      offerTransforms(current)
    } finally {
      if (!destroyed && operation === transformPendingEpoch) {
        transformPending = false
        transformPendingEpoch = undefined
      }
      if (!destroyed) {
        publish()
        Promise.resolve().then(onFocus)
      }
    }
  }

  function cancelTransform() {
    invalidateTransform()
    transformPending = false
    transformPendingEpoch = undefined
    error('')
    publish()
    Promise.resolve().then(onFocus)
  }

  function cleanup() {
    destroyed = true
    invalidatePolls()
    clearTimeout(completionTimer)
    clearTimeout(eligibleTimer)
    unlisten?.()
  }

  publish()
  return { start, stop, transform, busy, invalidateTransform, cancelTransform, snapshot, cleanup }
}
