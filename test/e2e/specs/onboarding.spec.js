import path from 'node:path'
import { access, appendFile, mkdir, readFile } from 'node:fs/promises'
import { chooseFolder } from '../support/folder-dialog.mjs'
import { homePathMatches } from '../support/home-path.mjs'

const FOLDER_DIALOG_WAIT_SECONDS = 30
const FOLDER_DIALOG_TITLE = '(Select|Open|Choose|Pick).*([Ff]older|[Dd]irectory|[Ff]ile)'

// The JUnit report is the one diagnostic that survives a failed artifact
// upload. A wait that runs out names the desktop client connection and the
// rendered shell in its own message.
async function shellState() {
  let connection = 'unavailable'
  try {
    connection = JSON.stringify(await browser.execute(async () => (
      window.__TAURI__.core.invoke('attach_listener_status')
    ))) ?? 'unavailable'
  } catch {}
  let rendered = 'unavailable'
  try {
    rendered = (await (await $('main')).getText()).replace(/\s+/g, ' ').slice(0, 300) || 'empty'
  } catch {}
  return `desktop client status: ${connection}. shell: ${rendered}`
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
    await chooseFolder(
      home,
      FOLDER_DIALOG_WAIT_SECONDS,
      FOLDER_DIALOG_TITLE,
      process.env.MUNIMENT_E2E_RAW_DIR,
    )
    // WebDriver's getElementText is unreliable for this element here.
    // textContent provides a stable read.
    await browser.waitUntil(async () => await homePathMatches(location, home), {
      timeoutMsg: 'the Home picker DOM value did not match the isolated Home',
    })
    expect(await location.getProperty('textContent')).toBe(home)
    await (await $('[data-testid="onboarding-confirm"]')).click()
    const skipImport = await $('button=Continue without importing')
    await skipImport.waitForDisplayed()
    await skipImport.click()
    const signedOut = await $('button=Sign in')
    // The installed runtime answers the session, so this screen waits on the
    // desktop client connection. Report that connection when the wait runs out.
    try {
      await signedOut.waitForDisplayed({
        timeoutMsg: 'the signed-out screen did not appear after onboarding',
      })
    } catch (waitError) {
      throw new Error(`${waitError.message} ${await shellState()}`)
    }

    for (const directory of ['memory', 'agents', 'projects', 'sessions']) {
      expect(await readFile(path.join(home, directory, 'README.md'), 'utf8')).toContain(`# ${directory[0].toUpperCase()}${directory.slice(1)}`)
    }
  })
})
