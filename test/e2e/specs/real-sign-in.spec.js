import path from 'node:path'
import { appendFile, readFile } from 'node:fs/promises'
import { remote } from 'webdriverio'

const rawDir = process.env.MUNIMENT_E2E_RAW_DIR

describe('installed Linux nightly', () => {
  afterEach(async () => {
    const profile = await $('.profile-button')
    if (await profile.isExisting()) {
      await profile.click()
      await (await $('button=Sign out')).click()
      await (await $('button=Sign in')).waitForDisplayed({ timeout: 30000 })
    }
  })

  it('signs in through the production UI', async () => {
    const signedOut = await $('button=Sign in')
    await signedOut.waitForDisplayed()
    await browser.saveScreenshot(path.join(rawDir, '01-signed-out.png'))

    await signedOut.click()
    const username = process.env.MUNIMENT_E2E_USERNAME
    const password = process.env.MUNIMENT_E2E_PASSWORD
    if (!username || !password) throw new Error('dedicated E2E credentials are unavailable')

    const authUrlFile = process.env.MUNIMENT_E2E_AUTH_URL_FILE
    let authUrl
    await browser.waitUntil(async () => {
      try {
        authUrl = (await readFile(authUrlFile, 'utf8')).trim()
        return authUrl.startsWith('https://')
      } catch { return false }
    }, {
      timeout: 60000,
      timeoutMsg: 'production sign-in continuation was not opened',
    })
    const signInBrowser = await remote({
      hostname: '127.0.0.1', port: 9515, logLevel: 'error',
      capabilities: { browserName: 'chrome', 'goog:chromeOptions': { args: ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage'] } },
    })
    try {
      await signInBrowser.url(authUrl)
      const userField = await signInBrowser.$('input[type="email"], input[autocomplete="username"]')
      await userField.waitForDisplayed()
      await userField.setValue(username)
      let passwordField = await signInBrowser.$('input[type="password"], input[autocomplete="current-password"]')
      if (!await passwordField.isExisting()) {
        await (await signInBrowser.$('button[type="submit"]')).click()
        passwordField = await signInBrowser.$('input[type="password"], input[autocomplete="current-password"]')
      }
      await passwordField.waitForDisplayed()
      await passwordField.setValue(password)
      await (await signInBrowser.$('button[type="submit"]')).click()
      await signInBrowser.waitUntil(async () => (await signInBrowser.getUrl()).startsWith('http://127.0.0.1:'), { timeout: 120000 })
    } finally {
      await signInBrowser.deleteSession()
    }

    const authenticatedMarker = await $('textarea[placeholder="Ask anything"]')
    await authenticatedMarker.waitForDisplayed({ timeout: 120000 })
    await authenticatedMarker.saveScreenshot(path.join(rawDir, '02-authenticated.png'))

    let frontendLogs
    try {
      frontendLogs = await browser.getLogs('browser')
    } catch {
      throw new Error('frontend-console diagnostic channel unavailable')
    }
    await appendFile(path.join(rawDir, 'frontend-console.log'), frontendLogs.map(({ level, message }) => `${level}: ${message}\n`).join(''))

  })
})
