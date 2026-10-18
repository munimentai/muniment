// One provider sign-in, as the `local-mode-login` events move it: started,
// waiting in the browser, showing a device code, asking a question, then done,
// failed or cancelled. Models and the first run share it.

export function loginStart(provider, label = '') {
  return { provider, label, stage: 'start', message: 'Starting the sign-in…', url: '', code: '', prompt: null }
}

const ENDED = new Set(['done', 'failed', 'cancelled'])

export function loginEnded(login) {
  return !!login && ENDED.has(login.stage)
}

// The next sign-in state for one event, or null when the event belongs to
// another sign-in. `open` is a page to open in the browser, `stop` ends the
// event subscription, and `done` names a finished sign-in, with `pool` when
// the account joined the router's pool.
export function loginEvent(login, payload) {
  if (!login || (payload?.provider && payload.provider !== login.provider && payload.stage !== 'cancelled')) return null
  switch (payload.stage) {
    case 'event': {
      const event = payload.event ?? {}
      if (event.type === 'auth_url') return { login: { ...login, stage: 'browser', url: event.url, message: event.instructions ?? 'Continue in your browser.' }, open: event.url }
      if (event.type === 'device_code') {
        const url = event.verificationUriComplete ?? event.verificationUri ?? ''
        return { login: { ...login, stage: 'code', url, code: event.userCode ?? '', message: 'Enter this code on the sign-in page.' }, open: url || null }
      }
      return { login: event.message ? { ...login, message: event.message } : login }
    }
    case 'prompt':
      return { login: { ...login, prompt: payload } }
    case 'done':
      return { login: { ...login, stage: 'done', prompt: null }, stop: true, done: true, pool: !!payload.pool }
    case 'failed':
      return { login: { ...login, stage: 'failed', message: payload.message || 'The sign-in did not finish. Try again.', prompt: null }, stop: true }
    case 'cancelled':
      return { login: { ...login, stage: 'cancelled', message: 'The sign-in was cancelled.', prompt: null }, stop: true }
    case 'exit':
      if (loginEnded(login)) return { login }
      return { login: { ...login, stage: 'failed', message: 'The sign-in stopped before it finished. Try again.', prompt: null }, stop: true }
    default:
      return { login: payload.message ? { ...login, message: payload.message } : login }
  }
}
