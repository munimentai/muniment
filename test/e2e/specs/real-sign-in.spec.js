import path from 'node:path'
import { access, appendFile, readFile } from 'node:fs/promises'
import { spawn } from 'node:child_process'
import { remote } from 'webdriverio'

const rawDir = process.env.MUNIMENT_E2E_RAW_DIR

describe('installed nightly', () => {
  afterEach(async () => {
    const profile = await $('.profile-button')
    if (await profile.isExisting()) {
      await profile.click()
      await (await $('button=Sign out')).click()
      await (await $('button=Sign in')).waitForDisplayed({ timeout: 30000 })
    }
  })

  it('signs in through the production UI', async function () {
    this.timeout(360000)
    const location = await $('[data-testid="onboarding-home-path"]')
    await location.waitForDisplayed()
    const home = await location.getProperty('textContent')
    expect(path.isAbsolute(home)).toBe(true)
    let homeExists = true
    try { await access(home) } catch { homeExists = false }
    expect(homeExists).toBe(false)
    await (await $('[data-testid="onboarding-confirm"]')).click()
    const skipImport = await $('button=Continue without importing')
    await skipImport.waitForDisplayed()
    await skipImport.click()

    const signedOut = await $('button=Sign in')
    await signedOut.waitForDisplayed()
    const readmes = ['memory', 'agents', 'projects', 'sessions'].map((directory) => ({
      directory,
      path: path.join(home, directory, 'README.md'),
    }))
    await browser.waitUntil(async () => {
      try {
        await Promise.all(readmes.map(({ path: readme }) => readFile(readme, 'utf8')))
        return true
      } catch {
        return false
      }
    }, { timeoutMsg: 'Home README files were not created' })
    for (const { directory, path: readme } of readmes) {
      expect(await readFile(readme, 'utf8')).toContain(`# ${directory[0].toUpperCase()}${directory.slice(1)}`)
    }

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
    let authDriver
    let signInBrowser
    try {
      if (process.platform === 'win32') {
        // The Tauri service has already put the WebView2-matched Edge driver on
        // PATH. Reuse those exact test-side bytes for the hosted auth window.
        authDriver = spawn('msedgedriver.exe', ['--port=9515', '--allowed-ips=127.0.0.1', '--log-level=WARNING'], {
          stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true,
        })
        authDriver.stdout.pipe((await import('node:fs')).createWriteStream(path.join(rawDir, 'edge-auth-driver.log')))
        authDriver.stderr.pipe((await import('node:fs')).createWriteStream(path.join(rawDir, 'edge-auth-driver.log'), { flags: 'a' }))
        await browser.waitUntil(async () => {
          try { return (await fetch('http://127.0.0.1:9515/status')).ok } catch { return false }
        }, { timeout: 30000, timeoutMsg: 'matching Edge WebDriver did not start' })
      } else if (process.platform === 'linux') {
        authDriver = spawn('WebKitWebDriver', ['--port=9515'], { stdio: ['ignore', 'pipe', 'pipe'] })
        authDriver.stdout.pipe((await import('node:fs')).createWriteStream(path.join(rawDir, 'webkit-auth-driver.log')))
        authDriver.stderr.pipe((await import('node:fs')).createWriteStream(path.join(rawDir, 'webkit-auth-driver.log'), { flags: 'a' }))
        await browser.waitUntil(async () => {
          try { return (await fetch('http://127.0.0.1:9515/status')).ok } catch { return false }
        }, { timeout: 30000, timeoutMsg: 'WebKitWebDriver did not start' })
      }
      signInBrowser = await remote({
        hostname: '127.0.0.1', port: 9515, logLevel: 'silent',
        capabilities: process.platform === 'win32'
          ? { browserName: 'MicrosoftEdge', 'ms:edgeOptions': { args: ['--headless=new', '--disable-gpu'] } }
          : process.platform === 'linux'
            ? { browserName: 'MiniBrowser', 'wdio:enforceWebDriverClassic': true }
            : { browserName: 'chrome', 'goog:chromeOptions': { args: ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage'] } },
      })
      await signInBrowser.url(authUrl)
      const userField = await signInBrowser.$('input[type="email"], input[autocomplete="username"]')
      await userField.waitForDisplayed()
      await userField.setValue(username)
      let passwordField = await signInBrowser.$('input[type="password"], input[autocomplete="current-password"]')
      if (!await passwordField.isExisting()) {
        await signInBrowser.keys('Enter')
        passwordField = await signInBrowser.$('input[type="password"], input[autocomplete="current-password"]')
      }
      await passwordField.waitForDisplayed()
      await passwordField.setValue(password)
      await signInBrowser.keys('Enter')
      await signInBrowser.waitUntil(async () => (await signInBrowser.getUrl()).startsWith('http://127.0.0.1:'), { timeout: 120000 })
    } finally {
      try {
        if (signInBrowser) await signInBrowser.deleteSession()
      } finally {
        if (authDriver) authDriver.kill()
      }
    }

    const authenticatedMarker = await $('textarea[placeholder="Ask anything"]')
    await authenticatedMarker.waitForDisplayed({ timeout: 120000 })
    await authenticatedMarker.saveScreenshot(path.join(rawDir, '02-authenticated.png'))
    const prompt = `Muniment E2E chat ${Date.now()}`
    await authenticatedMarker.setValue(prompt)
    const send = await $('button=Send')
    await send.waitForDisplayed()
    await send.click()

    const userMessage = await $(`//div[contains(concat(' ', normalize-space(@class), ' '), ' user-turn ')]//p[normalize-space()="${prompt}"]`)
    await userMessage.waitForDisplayed()
    const response = await userMessage.$('./ancestor::div[contains(concat(" ", normalize-space(@class), " "), " user-turn ")]/following-sibling::div[contains(concat(" ", normalize-space(@class), " "), " response ")][1]')
    let receipt
    await browser.waitUntil(async () => {
      receipt = await response.$('button.provenance')
      return await receipt.isDisplayed()
    }, {
      timeout: 180000,
      timeoutMsg: `chat response did not complete with a receipt for prompt: ${prompt}`,
    })

    const assistantResponse = await response.$('./p[1]')
    const assistantText = (await assistantResponse.getText()).trim()
    expect(assistantText).not.toBe('')
    await receipt.click()
    const route = await response.$('.route-value')
    await route.waitForDisplayed()
    expect((await route.getText()).trim()).not.toBe('')

    let frontendLogs
    try {
      frontendLogs = await browser.getLogs('browser')
    } catch {
      throw new Error('frontend-console diagnostic channel unavailable')
    }
    await appendFile(path.join(rawDir, 'frontend-console.log'), frontendLogs.map(({ level, message }) => `${level}: ${message}\n`).join(''))

  })
})
