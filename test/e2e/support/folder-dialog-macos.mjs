// The E2E app sends AppKit events to its own NSOpenPanel. No privacy grant
// or external UI automation process participates in this drive.
export async function driveMacosFolder(waitSeconds, poll = () => browser.execute(
  () => window.__TAURI__.core.invoke('e2e_drive_folder_dialog'),
)) {
  let timer
  let stopped = false
  try {
    await Promise.race([
      (async () => {
        while (!stopped) {
          const closed = await poll()
          if (closed === true) return
          if (closed !== false) throw new Error('The AppKit picker drive returned an invalid state.')
          if (!stopped) await new Promise((resolve) => setTimeout(resolve, 100))
        }
      })(),
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error('The NSOpenPanel timed out during the AppKit picker drive.')), waitSeconds * 1000)
      }),
    ])
  } finally {
    stopped = true
    clearTimeout(timer)
  }
}
