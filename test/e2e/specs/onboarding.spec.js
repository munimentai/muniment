import path from 'node:path'
import { access, appendFile, mkdir, readFile } from 'node:fs/promises'
import { chooseFolder, folderDialogDescription, recordMacosPickerBaseline, withWindowsPickerDiagnostics } from '../support/onboarding-folder.mjs'
import { homePathMatches } from '../support/home-path.mjs'
import { expandSidebar, openFirstRunModelSettings, openSettingsPage } from '../support/first-run.mjs'

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
  it('opens at or above the configured minimum window size', async () => {
    const config = JSON.parse(await readFile(new URL('../../../src-tauri/tauri.conf.json', import.meta.url), 'utf8'))
    const { minWidth, minHeight } = config.app.windows[0]
    await (await $('main')).waitForDisplayed({ timeout: 120000 })
    const size = await browser.execute(() => ({ width: window.innerWidth, height: window.innerHeight }))
    expect(size.width).toBeGreaterThanOrEqual(minWidth)
    expect(size.height).toBeGreaterThanOrEqual(minHeight)
  })

  it('chooses an isolated Home and scaffolds its README files', async () => {
    const home = process.env.MUNIMENT_E2E_HOME_PATH
    const location = await $('[data-testid="onboarding-home-path"]')
    let profileDirectory = { error: 'The app profile directory is unavailable.' }
    try {
      profileDirectory = await browser.execute(async () => {
        try {
          const result = await Promise.race([
            window.__TAURI__.path.appConfigDir(),
            new Promise((_, reject) => setTimeout(() => reject('The app profile directory query timed out.'), 5000)),
          ])
          return { result }
        } catch (error) {
          return { error: String(error) }
        }
      })
    } catch {}
    await appendFile(
      path.join(process.env.MUNIMENT_E2E_RAW_DIR, 'onboarding-first-render.log'),
      `profile_directory: ${JSON.stringify(profileDirectory)}\n`,
    )
    try {
      await location.waitForDisplayed({
        timeout: 120000,
        timeoutMsg: 'model-ready onboarding first render did not show the Home chip',
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

    // The first run asks one question: which model to connect.
    const question = await $('h1=Connect a model')
    await question.waitForDisplayed({ timeout: 120000 })
    expect(await (await $('button=More providers')).isEnabled()).toBe(true)
    expect((await $$('.connect .options button.option')).length).toBeGreaterThanOrEqual(9)
    expect(await (await $('textarea[placeholder="Ask anything"]')).isExisting()).toBe(false)
    expect(await (await $('[data-testid="onboarding-confirm"]')).isExisting()).toBe(false)
    // Include More providers and the Settings checks in the failure report.
    try {
      await openFirstRunModelSettings()
      const localMode = await $('[data-testid="local-mode"]')
      await localMode.waitForDisplayed({
        timeoutMsg: 'model settings did not appear after More providers',
      })
    } catch (waitError) {
      let failure = 'Muniment could not read the first-run screen.'
      try {
        const firstRun = await $('section.onboarding')
        failure = await firstRun.isExisting() ? (await firstRun.getText()).replace(/\s+/g, ' ').slice(0, 300) : 'The first-run screen is closed.'
      } catch {}
      throw new Error(`${waitError.message} First run: ${failure} ${await shellState()}`)
    }
    const settings = await $('[role="dialog"][aria-label="Settings"]')
    await settings.waitForDisplayed()
    expect(await (await settings.$('h4=Connect account')).isDisplayed()).toBe(true)

    // The first run shows Home with no control. Settings → Data → Storage
    // changes it, and Home settings closes Settings.
    await openSettingsPage('Data')
    const storage = await settings.$('[data-settings-part="storage"]')
    await storage.waitForDisplayed({ timeoutMsg: 'Settings → Data did not show Storage.' })
    const change = await storage.$('button=Change workspace folder…')
    await change.waitForDisplayed()
    await change.click()
    const homeSettings = await $('section[aria-label="Home settings"]')
    await homeSettings.waitForDisplayed()
    await mkdir(home, { recursive: true })
    // Record any system prompt before the app opens NSOpenPanel.
    await recordMacosPickerBaseline(process.env.MUNIMENT_E2E_RAW_DIR)
    await (await $('[data-testid="onboarding-picker"]')).click()
    await withWindowsPickerDiagnostics(async () => {
      await chooseFolder(
        home,
        FOLDER_DIALOG_WAIT_SECONDS,
        FOLDER_DIALOG_TITLE,
        process.env.MUNIMENT_E2E_RAW_DIR,
      )
      // Home settings show the path, and data-home-path holds it in full.
      let location
      await browser.waitUntil(async () => {
        location = await $('[data-testid="onboarding-home-path"]')
        return await homePathMatches(location, home)
      }, {
        timeoutMsg: `The Home picker DOM value did not match the isolated Home. ${folderDialogDescription(FOLDER_DIALOG_TITLE)}`,
      })
      expect(await location.getAttribute('data-home-path')).toBe(home)
    }, () => browser.execute(() => document.querySelector('[data-home-picker]')?.getAttribute('data-home-picker')),
    process.env.MUNIMENT_E2E_RAW_DIR)
    for (const directory of ['memory', 'agents', 'projects', 'sessions']) {
      let exists = true
      try { await access(path.join(home, directory)) } catch { exists = false }
      expect(exists).toBe(false)
    }
    await (await $('button=Save Home')).click()
    await homeSettings.waitForDisplayed({ reverse: true, timeout: 10000 })
    await settings.waitForDisplayed({ reverse: true, timeout: 10000 })
    expect(await (await $('section[aria-label="First run"]')).isExisting()).toBe(false)

    await expandSidebar()
    const row = await $('header.titlebar')
    expect((await row.getSize()).height).toBe(35)
    const newThread = await $('#sidebar .side-top > button:first-child')
    expect(await newThread.getAttribute('aria-label')).toBe('New thread')
    expect(await newThread.isDisplayed()).toBe(true)
    expect((await newThread.getSize()).height).toBeGreaterThanOrEqual(24)
    for (const name of ['Collapse sidebar', 'Rename thread', 'Workspace tools']) {
      const control = await row.$(`button[aria-label="${name}"]`)
      expect(await control.isDisplayed()).toBe(true)
      const size = await control.getSize()
      expect(size.width).toBeGreaterThanOrEqual(24)
      expect(size.height).toBeGreaterThanOrEqual(24)
    }
    expect(await (await $('#sidebar button[aria-label="Artifacts"]')).isDisplayed()).toBe(true)
    expect(await (await row.$('.update-slot')).getProperty('childElementCount')).toBe(0)
    expect(await row.getAttribute('data-tauri-drag-region')).not.toBeNull()
    expect(await (await $('textarea[placeholder="Ask anything"]')).isDisplayed()).toBe(true)
    for (const directory of ['memory', 'agents', 'projects', 'sessions']) {
      expect(await readFile(path.join(home, directory, 'README.md'), 'utf8')).toContain(`# ${directory[0].toUpperCase()}${directory.slice(1)}`)
    }
  })
})
