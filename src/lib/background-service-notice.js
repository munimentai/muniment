// The runtime drops a chat-event subscriber whose queue fills, and the desktop
// resubscribes after a 250 millisecond retry. A drop that short must leave the
// workspace on screen, so the notice waits out a two second dwell first.
const dwell = 2000

function serviceUnreachable(status) {
  return Boolean(status?.supervisor_running)
    && (status.connected === false || status.chat_events_connected === false)
}

export function createBackgroundServiceNotice({
  setTimer = setTimeout,
  clearTimer = clearTimeout,
  onVisible,
}) {
  let timer
  let visible = false
  let readAnyStatus = false

  function show(next) {
    if (visible === next) return
    visible = next
    onVisible(next)
  }

  // A null status means the window has read no status yet.
  function update(status) {
    if (!status) return
    const first = !readAnyStatus
    readAnyStatus = true

    if (!serviceUnreachable(status)) {
      clearTimer(timer)
      timer = undefined
      show(false)
      return
    }
    // A window that opens onto an outage owes the user the notice at once.
    if (first) {
      show(true)
      return
    }
    // The dwell measures the outage from its first status. A repeat status
    // during the dwell never extends it.
    if (visible || timer !== undefined) return
    timer = setTimer(() => {
      timer = undefined
      show(true)
    }, dwell)
  }

  function cleanup() {
    clearTimer(timer)
    timer = undefined
  }

  return { update, cleanup }
}
