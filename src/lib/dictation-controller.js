import { appendTranscript, isDictationActive } from './dictation-state.js'

const transcriptQuietPeriod = 25

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
  let destroyed = false

  function snapshot() {
    return {
      status,
      commandPending,
      requested,
      finishing,
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
    return commandPending || finishing || isDictationActive(status)
  }

  function stopPolling() {
    clearTimeout(pollTimer)
    pollTimer = undefined
  }

  function invalidatePolls() {
    pollEpoch += 1
    stopPolling()
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
    if (!cancelled) Promise.resolve().then(onFocus)
  }

  function waitForTranscriptQuiet(epoch) {
    clearTimeout(completionTimer)
    completionTimer = setTimeout(() => complete(epoch), transcriptQuietPeriod)
  }

  function finish(epoch) {
    clearTimeout(completionTimer)
    completionTimer = setTimeout(() => waitForTranscriptQuiet(epoch))
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
    cancelled = false
    draftSnapshot = readDraft()
    transcript = ''
    captureEpoch += 1
    completionEpoch = undefined
    finishing = false
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

  function cleanup() {
    destroyed = true
    invalidatePolls()
    clearTimeout(completionTimer)
    unlisten?.()
  }

  publish()
  return { start, stop, busy, snapshot, cleanup }
}
