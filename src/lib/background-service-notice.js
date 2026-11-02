const messages = {
  starting: 'The runtime received a start request.',
  connected: '',
  disconnected: 'The runtime connection closed.',
  exited: 'The runtime exited.',
  startFailed: 'The runtime start failed.',
  requiresApproval: 'The runtime registration needs approval in System Settings > General > Login Items.',
  approved: 'The runtime registration has approval.',
  notFound: 'The runtime registration found no service.',
  registrationFailed: 'The runtime registration failed.',
  childStarted: 'The desktop runs the runtime while the background service is off.',
  stopped: 'The runtime stopped.',
  stopFailed: 'The runtime stop failed.',
}

// The runtime runs as the desktop's child because the background service could
// not stay running, so it stops when the app quits.
const foregroundOnly = {
  text: 'Scheduled agents run only while Muniment is open.',
  control: 'Open Login Items',
  command: 'open_login_items',
}

// A child that runs because Login Items keeps the service off speaks with the
// Login Items notice, since the user turned the service off and nothing failed.
// While the child replaces the stopped service, the workspace stays and shows
// that it reconnects.
export function runtimeNotice(state) {
  if (!state || !Object.hasOwn(messages, state.lastEvent)) return null
  const cause = state.lastEvent === 'startFailed' && typeof state.cause === 'string' ? state.cause.trim() : ''
  const visible = state.lastEvent !== 'connected' && state.visible === true
  const loginItems = state.lastEvent === 'requiresApproval' || (state.lastEvent === 'childStarted' && state.foregroundOnly === true)
  return {
    visible,
    text: state.lastEvent === 'childStarted' && loginItems ? foregroundOnly.text : messages[state.lastEvent] + (cause ? ` ${cause}` : ''),
    control: loginItems ? 'Open Login Items' : 'Start runtime',
    command: loginItems ? 'open_login_items' : 'runtime_start',
    busy: state.busy === true,
    reconnecting: !visible && state.reconnecting === true,
    foregroundOnly: state.foregroundOnly === true ? foregroundOnly : null,
  }
}

// Windows read the shell owner's snapshot. No window owns a lifecycle timer.
export function createBackgroundServiceNotice({ invoke, listen, onChange, setPoll = setInterval, clearPoll = clearInterval }) {
  let revision = -1
  let started = false
  let stopped = false
  let unlisten
  let poll
  let current
  let pending = false

  function update(state) {
    if (stopped || !Number.isSafeInteger(state?.revision) || state.revision < 0 || state.revision < revision) return
    const notice = runtimeNotice(state)
    if (!notice) return
    revision = state.revision
    current = notice
    onChange(notice)
  }

  async function read() {
    try { update(await invoke('runtime_state')) } catch { console.error('The runtime status failed.') }
  }

  async function start() {
    if (started || stopped) return
    started = true
    try {
      const stop = await listen('runtime-state-changed', ({ payload }) => update(payload))
      if (stopped) { stop?.(); return }
      unlisten = stop
    } catch { console.error('The runtime listener registration failed.') }
    if (stopped) return
    poll = setPoll(read, 1000)
    await read()
  }

  async function retry() {
    if (stopped || pending || !current || current.busy) return
    pending = true
    try { await invoke(current.command) } catch { console.error('The runtime control failed.') }
    finally {
      pending = false
      if (!stopped) await read()
    }
  }

  async function openLoginItems() {
    try { await invoke(foregroundOnly.command) } catch { console.error('The runtime control failed.') }
  }

  function cleanup() {
    stopped = true
    clearPoll(poll)
    unlisten?.()
  }

  return { start, retry, openLoginItems, cleanup }
}
