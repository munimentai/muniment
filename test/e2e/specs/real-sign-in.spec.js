import path from 'node:path'
import { access, appendFile, readFile } from 'node:fs/promises'
import { spawn } from 'node:child_process'
import { remote } from 'webdriverio'
import { withAuthDiagnostics } from '../support/auth-diagnostics.mjs'
import { expandSidebar, openFirstRunModelSettings } from '../support/first-run.mjs'

const rawDir = process.env.MUNIMENT_E2E_RAW_DIR

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

async function verifyInitialHome(location) {
  await browser.waitUntil(async () => path.isAbsolute(await location.getProperty('textContent')))
  const home = await location.getProperty('textContent')
  expect(path.isAbsolute(home)).toBe(true)
  let homeExists = true
  try { await access(home) } catch { homeExists = false }
  expect(homeExists).toBe(false)
  return home
}

const hostedLogin = 'button[name="action"][value="login"]'
const hostedSelect = 'button[name="action"][value="select"]'
const hostedApprove = 'button[name="action"][value="approve"]'

async function hostedDisplayed(driver, selector) {
  try {
    return await (await driver.$(selector)).isDisplayed()
  } catch {
    return false
  }
}

async function hostedLocation(driver) {
  try {
    return await driver.getUrl()
  } catch {
    return ''
  }
}

async function signInCompleted(driver) {
  if (process.platform === 'linux') {
    try {
      return await (await $('.profile-button')).isDisplayed()
    } catch {
      return false
    }
  }
  return (await hostedLocation(driver)).startsWith('http://127.0.0.1:')
}

async function waitForLinuxSignIn(driver) {
  if (process.platform !== 'linux') return false
  try {
    // Give the callback its full budget before another command can block on WebKitWebDriver.
    await browser.waitUntil(async () => await signInCompleted(driver), { timeout: 120000 })
    return true
  } catch {
    return false
  }
}

async function activateHosted(driver, selector) {
  if (process.platform === 'linux') {
    // WebKitWebDriver holds the session when a click starts a navigation.
    // Schedule the click after Execute Script returns.
    let clicked = false
    try {
      clicked = await driver.execute((sel) => {
        const el = document.querySelector(sel)
        if (!el) return false
        setTimeout(() => el.click(), 0)
        return true
      }, selector)
    } catch (error) {
      const message = error instanceof Error ? error.message : ''
      if (!/timeout/i.test(message)) throw error
      // A page-load timeout after the click is not a failed click.
      clicked = true
    }
    if (!clicked) throw new Error(`production control ${selector} was not found`)
    return
  }
  const control = await driver.$(selector)
  await control.waitForDisplayed()
  await control.click()
}

describe('installed nightly', () => {
  afterEach(async () => {
    const profile = await $('.profile-button')
    if (await profile.isExisting()) {
      await profile.click()
      await (await $('button=Sign out')).click()
      // A sign-out lands in local mode.
      await (await $('[data-testid="local-mode"]')).waitForDisplayed({ timeout: 30000 })
    }
  })

  it('signs in through the production UI', withAuthDiagnostics(async function () {
    const location = await $('[data-testid="onboarding-home-path"]')
    const signedOut = await $('button=Sign in')
    const localMode = await $('[data-testid="local-mode"]')
    await browser.waitUntil(async () => (
      await location.isDisplayed() || await signedOut.isDisplayed() || await localMode.isDisplayed()
    ), {
      timeout: 120000,
      timeoutMsg: 'onboarding, the signed-out screen, and Local mode did not appear',
    })
    let home
    if (await location.isDisplayed()) {
      home = await verifyInitialHome(location)
      const composer = await $('textarea[placeholder="Ask anything"]')
      expect(await composer.isDisplayed()).toBe(true)
      await composer.setValue('Help me organize my notes.')
      await openFirstRunModelSettings()
    }

    // The desktop client can restore Local mode after onboarding.
    try {
      await browser.waitUntil(async () => (
        await signedOut.isDisplayed() || await localMode.isDisplayed()
      ), {
        timeout: 120000,
        timeoutMsg: 'the signed-out screen and Local mode did not appear after onboarding',
      })
    } catch (waitError) {
      throw new Error(`${waitError.message} ${await shellState()}`)
    }
    const inLocalMode = await localMode.isDisplayed()
    if (inLocalMode) await expandSidebar()
    let signIn = signedOut
    if (inLocalMode) {
      // The cloud sign-in sits in the Settings menu at the foot of the sidebar.
      await expandSidebar()
      await (await $('button=Settings')).click()
      // Settings opens on Models; the cloud sign-in is the Account section's one action.
      const sections = await $('nav[aria-label="Settings sections"]')
      await sections.waitForDisplayed()
      await (await sections.$('button=Account')).click()
      signIn = await $('button=Sign in for cloud features')
      await signIn.waitForDisplayed()
    }
    await signIn.waitForDisplayed()

    if (home) {
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
    }

    await browser.saveScreenshot(path.join(rawDir, '01-signed-out.png'))

    if (process.platform === 'linux') {
      // WebKitGTK can acknowledge a native click without dispatching it.
      await browser.execute((control) => control.click(), signIn)
    } else {
      await signIn.click()
    }
    const username = process.env.MUNIMENT_E2E_USERNAME
    const password = process.env.MUNIMENT_E2E_PASSWORD
    if (!username || !password) throw new Error('dedicated E2E credentials are unavailable')

    // The runtime announces the link before it opens a browser, and the shell
    // shows it, so the spec reads the link the user sees on every platform.
    const signInLink = browser.$('[data-testid="sign-in-link"]')
    let authUrl
    await browser.waitUntil(async () => {
      try {
        authUrl = ((await signInLink.getAttribute('href')) || '').trim()
        return authUrl.startsWith('https://')
      } catch { return false }
    }, {
      timeout: 120000,
      timeoutMsg: 'production sign-in link did not appear',
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
        // A WebKit navigation can hold each command while the page changes.
        // Keep those expected timeouts below the surrounding sign-in waits.
        ...(process.platform === 'linux'
          ? { connectionRetryTimeout: 5000, connectionRetryCount: 0 }
          : {}),
        capabilities: process.platform === 'win32'
          ? { browserName: 'MicrosoftEdge', 'ms:edgeOptions': { args: ['--headless=new', '--disable-gpu'] } }
          : process.platform === 'linux'
            ? { browserName: 'MiniBrowser', pageLoadStrategy: 'none', 'wdio:enforceWebDriverClassic': true }
            : { browserName: 'chrome', 'goog:chromeOptions': { args: ['--headless=new', '--no-sandbox', '--disable-dev-shm-usage'] } },
      })
      if (process.platform === 'linux') {
        // WebKitWebDriver never finishes Navigate on the continuation.
        // Assign the URL from the blank page so the session stays responsive.
        try {
          await signInBrowser.setTimeout({ pageLoad: 15000, implicit: 0 })
        } catch {}
        try {
          await signInBrowser.execute((target) => {
            window.location.assign(target)
          }, authUrl)
        } catch {
          // A page-load timeout is not a failed open if the login page appears.
        }
      } else {
        await signInBrowser.url(authUrl)
      }
      await (await signInBrowser.$('main[data-native-authorization="pending"]')).waitForDisplayed({
        timeout: 60000,
        timeoutMsg: 'production authorization login page did not appear',
      })
      const userField = await signInBrowser.$('input[type="email"], input[autocomplete="username"]')
      await userField.waitForDisplayed({
        timeout: 30000,
        timeoutMsg: 'production authorization email field did not appear',
      })
      await userField.setValue(username)
      let passwordField = await signInBrowser.$('input[type="password"], input[autocomplete="current-password"]')
      if (!await passwordField.isExisting()) {
        await activateHosted(signInBrowser, hostedLogin)
        passwordField = await signInBrowser.$('input[type="password"], input[autocomplete="current-password"]')
      }
      await passwordField.waitForDisplayed({
        timeout: 30000,
        timeoutMsg: 'production authorization password field did not appear',
      })
      await passwordField.setValue(password)
      await activateHosted(signInBrowser, hostedLogin)
      if (!(await waitForLinuxSignIn(signInBrowser))) {
        await signInBrowser.waitUntil(async () => (
          await signInCompleted(signInBrowser)
          || await hostedDisplayed(signInBrowser, hostedSelect)
          || await hostedDisplayed(signInBrowser, hostedApprove)
        ), {
          timeout: 60000,
          timeoutMsg: 'production authorization did not continue after local sign-in',
        })
      }
      if (!(await signInCompleted(signInBrowser))
        && await hostedDisplayed(signInBrowser, hostedSelect)) {
        await activateHosted(signInBrowser, hostedSelect)
        if (!(await waitForLinuxSignIn(signInBrowser))) {
          await signInBrowser.waitUntil(async () => (
            await signInCompleted(signInBrowser)
            || await hostedDisplayed(signInBrowser, hostedApprove)
          ), {
            timeout: 30000,
            timeoutMsg: 'production authorization did not continue after organization choice',
          })
        }
      }
      if (!(await signInCompleted(signInBrowser))) {
        await (await signInBrowser.$(hostedApprove)).waitForDisplayed({
          timeout: 15000,
          timeoutMsg: 'production authorization did not ask for approval',
        })
        await activateHosted(signInBrowser, hostedApprove)
      }
      const completionDriver = process.platform === 'linux' ? browser : signInBrowser
      await completionDriver.waitUntil(async () => await signInCompleted(signInBrowser), {
        timeout: 120000,
        timeoutMsg: 'production sign-in did not return to the desktop callback',
      })
    } finally {
      try {
        // WebKitWebDriver can hold Delete Session after the callback navigation.
        // Stopping its process releases the session without blocking the test.
        if (signInBrowser && process.platform !== 'linux') await signInBrowser.deleteSession()
      } catch (error) {
        console.error('Failed to delete hosted sign-in session.', error)
      } finally {
        if (authDriver) authDriver.kill()
      }
    }

    const authenticatedMarker = await $('textarea[placeholder="Ask anything"]')
    await authenticatedMarker.waitForDisplayed({ timeout: 120000 })
    await authenticatedMarker.saveScreenshot(path.join(rawDir, '02-authenticated.png'))
    await browser.waitUntil(async () => {
      const attachStatus = await browser.execute(async () => (
        window.__TAURI__.core.invoke('attach_listener_status')
      ))
      return attachStatus.supervisor_running === true && attachStatus.connected === true
    }, {
      timeout: 60000,
      timeoutMsg: 'desktop client did not connect to the installed runtime',
    })
    await authenticatedMarker.waitForEnabled({
      timeout: 60000,
      timeoutMsg: 'composer did not become ready after sign-in and thread restore',
    })
    const prompt = `Muniment E2E chat ${Date.now()}`
    await authenticatedMarker.setValue(prompt)
    const send = await $('button[aria-label="Send"]')
    await send.waitForDisplayed()
    await browser.waitUntil(async () => (
      await send.isEnabled() && await send.getAttribute('aria-disabled') !== 'true'
    ), {
      timeout: 60000,
      timeoutMsg: 'Send did not become ready after sign-in and thread restore',
    })
    await send.click()

    const userMessage = await $(`//div[contains(concat(' ', normalize-space(@class), ' '), ' user-turn ')]//p[normalize-space()="${prompt}"]`)
    await userMessage.waitForDisplayed()
    const response = await userMessage.$('./ancestor::div[contains(concat(" ", normalize-space(@class), " "), " user-turn ")]/following-sibling::div[contains(concat(" ", normalize-space(@class), " "), " response ")][1]')
    let receipt
    let refusal
    await browser.waitUntil(async () => {
      const failure = await response.$('.run-error')
      if (await failure.isDisplayed()) {
        refusal = await failure.getText()
        return true
      }
      receipt = await response.$('button.provenance')
      return await receipt.isDisplayed()
    }, {
      timeout: 180000,
      timeoutMsg: `The chat response returned neither a receipt nor a refusal for prompt: ${prompt}`,
    })
    if (refusal !== undefined) {
      throw new Error(`The chat response failed: ${refusal}`)
    }

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

  }, rawDir)).timeout(900000)
})
