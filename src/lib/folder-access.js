// A launch reads Home beside the runtime's start, never on its path. macOS
// holds a read of Documents, Desktop or Downloads until the user answers its
// folder prompt, so a pending or refused prompt shows as its own state with
// core's words, and the watch reads again until Home opens.
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

export function createFolderAccessWatch({ invoke, homePath, onChange, wait = sleep, deniedPoll = 5000 }) {
  let stopped = false
  let running = false
  let current = null

  function publish(next) {
    current = next
    if (!stopped) onChange(next)
  }

  // A pending prompt answers `waiting` after a short bound. The next read
  // waits for the answer itself, so one read stays open at a time.
  async function read(waitForAnswer) {
    try { return await invoke('home_folder_state', { homePath, wait: waitForAnswer }) } catch { return null }
  }

  async function run() {
    if (running || stopped || !homePath) return
    running = true
    try {
      let waitForAnswer = false
      while (!stopped) {
        const answer = await read(waitForAnswer)
        if (stopped) return
        if (answer?.state !== 'waiting' && answer?.state !== 'denied') { publish(null); return }
        publish({ state: answer.state, folder: answer.folder ?? null, message: answer.message ?? '' })
        waitForAnswer = answer.state === 'waiting'
        if (answer.state === 'denied') await wait(deniedPoll)
      }
    } finally {
      running = false
    }
  }

  // Allow opens Files & Folders for a refusal, and otherwise reads Home
  // again, which raises the prompt once more if macOS dropped it.
  async function allow() {
    if (current?.state === 'denied') {
      try { await invoke('home_folder_settings') } catch { console.error('System Settings could not open.') }
    } else {
      const answer = await read(false)
      if (!stopped && answer?.state === 'open') publish(null)
    }
    void run()
  }

  function stop() {
    stopped = true
  }

  return { start: run, allow, stop }
}

// The card's heading names the protected folder, or Home when none holds it.
export function folderAccessHeading(access) {
  const folder = access?.folder || 'Home'
  return access?.state === 'denied' ? `No access to ${folder}` : `Waiting for ${folder} access`
}
