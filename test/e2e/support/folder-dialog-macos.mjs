// The E2E app sends AppKit events to its own NSOpenPanel. No privacy grant
// or external UI automation process participates in this drive.
async function snapshot() {
  return browser.execute(async () => {
    let timer
    let native
    try {
      native = await Promise.race([
        window.__TAURI__.core.invoke('e2e_folder_dialog_snapshot'),
        new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error('The AppKit snapshot timed out.')), 2000)
        }),
      ])
    } catch (error) {
      native = { error: error instanceof Error ? error.message : String(error) }
    } finally {
      clearTimeout(timer)
    }
    const outcome = document.querySelector('[data-home-picker]')?.getAttribute('data-home-picker')
    return {
      native,
      open: outcome ? JSON.parse(outcome) : { status: 'unavailable' },
    }
  })
}

export async function driveMacosFolder(waitSeconds, poll = () => browser.execute(
  () => window.__TAURI__.core.invoke('e2e_drive_folder_dialog'),
), diagnose = snapshot) {
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
  } catch (error) {
    stopped = true
    clearTimeout(timer)
    let diagnostics
    try {
      diagnostics = JSON.stringify(await Promise.race([
        diagnose(),
        new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error('The Home picker diagnostics timed out.')), 3000)
        }),
      ]))
    } catch (diagnosticError) {
      diagnostics = diagnosticError instanceof Error ? diagnosticError.message : String(diagnosticError)
    }
    const message = error instanceof Error ? error.message : String(error)
    throw new Error(`${message} Home picker diagnostics: ${diagnostics}`, { cause: error })
  } finally {
    stopped = true
    clearTimeout(timer)
  }
}
