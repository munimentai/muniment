import path from 'node:path'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { access, appendFile, mkdir, readFile } from 'node:fs/promises'

const run = promisify(execFile)
const FOLDER_DIALOG_WAIT_SECONDS = 30
const FOLDER_DIALOG_TITLE = '(Select|Open|Choose|Pick).*([Ff]older|[Dd]irectory|[Ff]ile)'

async function writeFolderDialogTimeoutArtifact() {
  const lines = [`searched title: ${FOLDER_DIALOG_TITLE}`, 'visible window titles:']
  try {
    const { stdout } = await run('xdotool', ['search', '--onlyvisible', '--name', '.*'])
    const windows = stdout.trim().split('\n').filter(Boolean)
    for (const window of windows) {
      try {
        const { stdout: title } = await run('xdotool', ['getwindowname', window])
        lines.push(`${window}: ${title.trim()}`)
      } catch {
        lines.push(`${window}: title unavailable`)
      }
    }
    if (windows.length === 0) lines.push('(none)')
  } catch (error) {
    lines.push(`window search failed: ${error.message}`)
  }
  await appendFile(
    path.join(process.env.MUNIMENT_E2E_RAW_DIR, 'folder-picker-timeout.log'),
    `${lines.join('\n')}\n`,
  )
}

async function chooseFolder(home) {
  let stdout
  try {
    ({ stdout } = await run('timeout', [
      String(FOLDER_DIALOG_WAIT_SECONDS), 'xdotool', 'search', '--sync', '--onlyvisible', '--name',
      FOLDER_DIALOG_TITLE,
    ]))
  } catch (error) {
    if (error.code === 124) {
      try { await writeFolderDialogTimeoutArtifact() } catch {}
    }
    throw error
  }
  const window = stdout.trim().split('\n').at(-1)
  await run('xdotool', ['windowfocus', '--sync', window])
  await run('xdotool', ['key', '--window', window, '--clearmodifiers', 'ctrl+l'])
  await run('xdotool', ['type', '--window', window, '--clearmodifiers', '--delay', '1', home])
  await run('xdotool', ['key', '--window', window, '--clearmodifiers', 'Return'])
  await new Promise((resolve) => setTimeout(resolve, 500))
  try {
    await run('xdotool', ['getwindowname', window])
  } catch {
    return
  }
  await run('xdotool', ['key', '--window', window, '--clearmodifiers', 'alt+s'])
}

describe('installed nightly model-ready onboarding', () => {
  it('chooses an isolated Home and scaffolds its README files', async () => {
    const home = process.env.MUNIMENT_E2E_HOME_PATH
    const location = await $('[data-testid="onboarding-home-path"]')
    try {
      await location.waitForDisplayed({
        timeout: 120000,
        timeoutMsg: 'model-ready onboarding first render did not show the Home picker',
      })
    } catch (waitError) {
      let homeStatus = { error: 'home_status diagnostic was unavailable' }
      try {
        homeStatus = await browser.execute(async () => {
          try {
            const result = await Promise.race([
              window.__TAURI__.core.invoke('home_status'),
              new Promise((_, reject) => setTimeout(() => reject('home_status timed out'), 5000)),
            ])
            return { result }
          } catch (error) {
            return { error: typeof error === 'string' ? error : 'home_status failed' }
          }
        })
      } catch {}
      let onboardingText = 'Onboarding section text was unavailable.'
      try {
        onboardingText = await (await $('section.onboarding')).getText()
      } catch {}
      try {
        await appendFile(
          path.join(process.env.MUNIMENT_E2E_RAW_DIR, 'onboarding-first-render.log'),
          `home_status: ${JSON.stringify(homeStatus)}\nonboarding text:\n${onboardingText}\n`,
        )
      } catch {}
      throw waitError
    }

    expect(path.isAbsolute(home)).toBe(true)
    let homeExists = true
    try { await access(home) } catch { homeExists = false }
    expect(homeExists).toBe(false)

    await mkdir(home, { recursive: true })
    await (await $('[data-testid="onboarding-picker"]')).click()
    await chooseFolder(home)
    await browser.waitUntil(async () => await location.getText() === home, {
      timeoutMsg: 'the Home picker did not select the isolated Home',
    })
    expect(await location.getText()).toBe(home)
    await (await $('[data-testid="onboarding-confirm"]')).click()
    const skipImport = await $('button=Continue without importing')
    await skipImport.waitForDisplayed()
    await skipImport.click()
    await (await $('button=Sign in')).waitForDisplayed()

    for (const directory of ['memory', 'agents', 'projects', 'sessions']) {
      expect(await readFile(path.join(home, directory, 'README.md'), 'utf8')).toContain(`# ${directory[0].toUpperCase()}${directory.slice(1)}`)
    }
  })
})
