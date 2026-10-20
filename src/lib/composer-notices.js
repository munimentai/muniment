// The composer's short notices: one shows at a time and the next follows once
// it leaves. A notice leaves on its own after `timeout` ms, and a hold, such as
// the pointer over it or keyboard focus in it, stops its clock until released.
// The clock runs only while a view shows the notice, so none leaves unseen.
// A notice shows once per launch for each cause: `show` with an id and cause
// that already showed does nothing, and a new cause shows it again.
export const COMPOSER_NOTICE_TIMEOUT = 6000

export function createComposerNotices({ timeout = COMPOSER_NOTICE_TIMEOUT, setTimer = (run, ms) => setTimeout(run, ms), clearTimer = (id) => clearTimeout(id), now = () => Date.now() } = {}) {
  const shown = new Map()
  const holds = new Set()
  const listeners = new Set()
  let queue = []
  let current = null
  let timer
  let remaining = timeout
  let armedAt = 0
  let views = 0
  let canExpire = () => true

  const emit = () => { for (const listener of listeners) listener(current) }

  function stopClock() {
    if (timer === undefined) return
    clearTimer(timer)
    timer = undefined
    remaining = Math.max(0, remaining - (now() - armedAt))
  }

  function runClock() {
    if (!current || timer !== undefined || holds.size || !views) return
    armedAt = now()
    timer = setTimer(expire, remaining)
  }

  // A notice a view keeps covered, such as under an open composer menu,
  // waits a full timeout once it is uncovered.
  function expire() {
    timer = undefined
    remaining = timeout
    if (canExpire()) advance()
    else runClock()
  }

  function present(notice) {
    current = notice
    remaining = timeout
    if (notice) shown.set(notice.id, notice.cause)
    runClock()
  }

  function advance() {
    stopClock()
    holds.clear()
    present(queue.shift() ?? null)
    emit()
  }

  function show(notice) {
    const next = { ...notice, cause: String(notice.cause ?? '') }
    if (current?.id === next.id) {
      if (current.cause === next.cause) return false
      stopClock()
      present(next)
      emit()
      return true
    }
    const queued = queue.findIndex((entry) => entry.id === next.id)
    if (queued >= 0) {
      queue[queued] = next
      return true
    }
    if (shown.get(next.id) === next.cause) return false
    queue.push(next)
    if (!current) advance()
    return true
  }

  // The notice's cause ended, so it leaves without counting as a dismissal.
  function withdraw(id) {
    queue = queue.filter((entry) => entry.id !== id)
    if (current?.id === id) advance()
  }

  function dismiss() {
    if (!current) return
    const notice = current
    advance()
    notice.ondismiss?.()
  }

  function act() {
    current?.action?.run()
  }

  function hold(reason) {
    if (!current) return
    holds.add(reason)
    stopClock()
  }

  function release(reason) {
    if (!holds.delete(reason)) return
    runClock()
  }

  // A view calls attach while it shows notices. `covered` reports whether
  // something lies over the notice, so its clock waits.
  function attach(covered = () => false) {
    views += 1
    canExpire = () => !covered()
    runClock()
    return () => {
      views -= 1
      if (!views) { stopClock(); canExpire = () => true }
    }
  }

  function subscribe(listener) {
    listeners.add(listener)
    listener(current)
    return () => listeners.delete(listener)
  }

  function cleanup() {
    stopClock()
    listeners.clear()
    queue = []
    current = null
  }

  return { show, withdraw, dismiss, act, hold, release, attach, subscribe, cleanup, current: () => current }
}
