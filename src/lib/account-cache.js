import { writable } from 'svelte/store'

// Scope snapshots to the app bridge. No account data goes into browser storage.
const caches = new WeakMap()
export function accountCache(tauri) {
  if (caches.has(tauri)) return caches.get(tauri)
  let current = { settings: null, error: '', refreshError: '' }
  const store = writable(current)
  let reading, refreshing, timer, users = 0, version = 0
  const publish = patch => { current = { ...current, ...patch }; store.set(current) }
  const set = settings => { version++; publish({ settings, error: '' }) }
  function read() {
    if (reading) return reading
    const started = version
    reading = Promise.resolve().then(() => tauri.invoke('model_router_settings')).then(settings => {
      if (started === version && settings?.accounts) set(settings)
      return current.settings
    }).catch(() => {
      publish({ error: 'Muniment could not read routing settings. Try again.' })
      return current.settings
    }).finally(() => { reading = null })
    return reading
  }
  function refresh() {
    if (refreshing) return refreshing
    refreshing = (async () => {
      await read()
      if (!current.settings?.accounts?.some(account => account.allowance_readable)) return
      const started = version
      try {
        const fresh = await tauri.invoke('model_router_refresh_quota', { id: null })
        if (started === version) {
          set({ ...current.settings, accounts: fresh.accounts })
          publish({ refreshError: '' })
        }
      } catch (_) {
        publish({ refreshError: 'Account allowances could not refresh. The last values remain visible.' })
      }
    })().finally(() => { refreshing = null })
    return refreshing
  }
  const cache = {
    subscribe: store.subscribe, get current() { return current }, set, read, refresh,
    start() {
      if (users++ === 0) {
        void refresh()
        timer = setInterval(() => { void refresh() }, 120_000)
      }
      return () => { if (--users === 0) clearInterval(timer) }
    },
  }
  caches.set(tauri, cache)
  return cache
}
