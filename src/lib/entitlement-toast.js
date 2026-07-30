const dismissAfter = 5000

export function createEntitlementToast({
  listen,
  setTimer = setTimeout,
  clearTimer = clearTimeout,
  onVisible,
}) {
  let timer
  let unlisten
  let destroyed = false

  function clear() {
    clearTimer(timer)
    timer = undefined
    if (!destroyed) onVisible(false)
  }

  function show() {
    if (destroyed) return
    clearTimer(timer)
    onVisible(true)
    timer = setTimer(() => {
      timer = undefined
      if (!destroyed) onVisible(false)
    }, dismissAfter)
  }

  async function start() {
    try {
      const stop = await listen('entitlement-changed', show)
      if (!stop) return
      if (destroyed) stop()
      else unlisten = stop
    } catch (_) {
      console.error('Entitlement listener registration failed.')
    }
  }

  function cleanup() {
    clear()
    destroyed = true
    unlisten?.()
    unlisten = undefined
  }

  return { start, clear, cleanup }
}
