import { handsFreeActivationDelay, isDictationActive } from './dictation-state.js'

export function createVoiceGesture({
  start,
  stop,
  readRequested,
  readStatus,
  busy,
  signedIn,
  hasActiveRun,
  now = () => Date.now(),
}) {
  let suppressClick = false
  let clickTimer
  let releaseTimer
  let releasePending = false
  let activationStartedAt
  let activationSource
  let pendingActivationAt
  let pendingActivationSource
  let handsFree = false
  let ignoreRelease = false
  let ignoreGlobalRelease = false
  let pointerId
  let key
  let globalHeld = false

  function expectClick() {
    suppressClick = true
    clearTimeout(clickTimer)
    clickTimer = setTimeout(() => { suppressClick = false })
  }

  function clearPendingRelease() {
    clearTimeout(releaseTimer)
    releaseTimer = undefined
    releasePending = false
  }

  function activate(source) {
    const activatedAt = now()
    activationStartedAt = activatedAt
    activationSource = source
    if (handsFree) {
      void stop()
      return true
    } else if (releasePending && (readRequested() || isDictationActive(readStatus())) && source === pendingActivationSource && activatedAt - pendingActivationAt <= handsFreeActivationDelay) {
      clearPendingRelease()
      handsFree = true
    } else if (readRequested() || isDictationActive(readStatus())) {
      void stop()
      return true
    } else void start()
    return false
  }

  function release(source, cancelled = false) {
    if (cancelled) {
      void stop(true)
      return
    }
    if (handsFree) return
    if (source !== activationSource || now() - activationStartedAt >= handsFreeActivationDelay) {
      void stop()
      return
    }
    releasePending = true
    pendingActivationAt = activationStartedAt
    pendingActivationSource = source
    clearTimeout(releaseTimer)
    releaseTimer = setTimeout(() => {
      releaseTimer = undefined
      releasePending = false
      void stop()
    }, handsFreeActivationDelay)
  }

  function pointerDown(event) {
    if (event.button !== 0 || pointerId !== undefined || key !== undefined) return
    event.preventDefault()
    expectClick()
    pointerId = event.pointerId
    event.currentTarget.setPointerCapture?.(event.pointerId)
    ignoreRelease = activate('pointer')
  }

  function pointerEnd(event) {
    if (event.pointerId !== pointerId) return
    event.preventDefault()
    expectClick()
    pointerId = undefined
    if (ignoreRelease) {
      ignoreRelease = false
      return
    }
    release('pointer', event.type === 'pointercancel')
  }

  function keyDown(event) {
    if (event.key !== ' ' && event.key !== 'Enter') return
    event.preventDefault()
    expectClick()
    if (!event.repeat) {
      if (key !== undefined || pointerId !== undefined) return
      key = event.key
      ignoreRelease = activate('keyboard')
    }
  }

  function keyUp(event) {
    if (event.key !== key) return
    event.preventDefault()
    expectClick()
    key = undefined
    if (ignoreRelease) {
      ignoreRelease = false
      return
    }
    release('keyboard')
  }

  function click() {
    if (suppressClick) {
      suppressClick = false
      clearTimeout(clickTimer)
      return
    }
    const skipRelease = activate('click')
    if (!skipRelease) release('click')
  }

  function globalShortcut({ state }) {
    if (state === 'Pressed') {
      if (globalHeld || !signedIn() || hasActiveRun() || (busy() && !releasePending && !handsFree)) return
      globalHeld = true
      ignoreGlobalRelease = activate('global')
    } else if (state === 'Released' && globalHeld) {
      globalHeld = false
      if (ignoreGlobalRelease) {
        ignoreGlobalRelease = false
        return
      }
      release('global')
    }
  }

  function inactive() {
    clearPendingRelease()
    handsFree = false
  }

  function cancel() {
    pointerId = undefined
    key = undefined
  }

  function cleanup() {
    clearTimeout(clickTimer)
    clearPendingRelease()
    globalHeld = false
  }

  return { pointerDown, pointerEnd, keyDown, keyUp, click, globalShortcut, inactive, cancel, cleanup }
}
