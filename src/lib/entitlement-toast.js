// Each entitlement change is a new cause, so it shows the composer notice
// again, or restarts its clock while it shows.
export const ENTITLEMENT_NOTICE = 'Your access changed. Some models or connections may differ.'

export function createEntitlementToast({ listen, notices }) {
  let unlisten
  let destroyed = false
  let changes = 0

  function clear() {
    notices.withdraw('entitlement')
  }

  function show() {
    if (destroyed) return
    changes += 1
    notices.show({ id: 'entitlement', cause: changes, text: ENTITLEMENT_NOTICE, dismissLabel: 'Dismiss access notice' })
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
