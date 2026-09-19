import path from 'node:path'
import { access, readFile } from 'node:fs/promises'
import { OLLAMA_BASE_URL } from '../support/local-provider.mjs'
import { piAgentDirectory } from '../support/pi-agent-directory.mjs'
import { expandSidebar, openFirstRunModelSettings } from '../support/first-run.mjs'

async function checkCandidatePackages() {
  if (process.env.MUNIMENT_PI_CANDIDATE !== '1') return
  const agentDirectory = piAgentDirectory()
  const packages = [
    ['pi-web-access', '0.28.0'],
    ['pi-subagents', '0.65.1'],
    ['pi-background-tasks', '2.5.0'],
    ['pi-mcp-adapter', '2.34.0'],
    ['pi-claude-bridge', '0.7.0'],
  ]
  const settings = JSON.parse(await readFile(path.join(agentDirectory, 'settings.json'), 'utf8'))
  expect(settings.packages).toEqual(packages.map(([name, version]) => `npm:${name}@${version}`))
  expect(settings.defaultTools).toEqual(['read', 'bash', 'powershell', 'edit', 'write', 'grep', 'find', 'ls'])
  for (const [name, version] of packages) {
    const manifest = JSON.parse(await readFile(path.join(agentDirectory, 'npm', 'node_modules', name, 'package.json'), 'utf8'))
    expect(manifest.version).toBe(version)
  }
}

async function completeOnboarding() {
  const location = await $('[data-testid="onboarding-home-path"]')
  await location.waitForDisplayed({
    timeout: 120000,
    timeoutMsg: 'local-mode onboarding did not show the Home path',
  })
  await browser.waitUntil(async () => path.isAbsolute(await location.getProperty('textContent')))
  const home = await location.getProperty('textContent')
  expect(path.isAbsolute(home)).toBe(true)
  let homeExists = true
  try { await access(home) } catch { homeExists = false }
  expect(homeExists).toBe(false)
  const composer = await $('textarea[placeholder="Ask anything"]')
  expect(await composer.isDisplayed()).toBe(true)
  await composer.setValue('Help me organize my notes.')
  await openFirstRunModelSettings()
}

async function waitForDesktopClient() {
  await browser.waitUntil(async () => {
    const status = await browser.execute(async () => (
      window.__TAURI__.core.invoke('attach_listener_status')
    ))
    return status.supervisor_running === true && status.connected === true
  }, {
    timeout: 60000,
    timeoutMsg: 'desktop client did not connect before the local-mode prompt',
  })
}

describe('installed local-mode chat', () => {
  afterEach(async () => {
    try {
      await browser.execute(async () => window.__TAURI__.core.invoke('local_mode_leave'))
    } catch (error) {
      console.error('Local mode cleanup failed.', error)
    }
  })

  it('starts a reply without cloud sign-in', async () => {
    await completeOnboarding()
    const localMode = await $('[data-testid="local-mode"]')
    await localMode.waitForDisplayed({ timeout: 120000 })
    await expandSidebar()

    const composer = await $('textarea[placeholder="Ask anything"]')
    await composer.waitForDisplayed({ timeout: 120000 })
    expect(await composer.getValue()).toBe('Help me organize my notes.')
    // The first Send opens Settings → Models; the composer chip's picker reopens it through Manage models.
    const settings = await $('[role="dialog"][aria-labelledby="settings-title"]')
    if (await settings.isDisplayed()) {
      await (await settings.$('button[aria-label="Close settings"]')).click()
      await settings.waitForDisplayed({ reverse: true, timeout: 10000 })
    }
    const emptyChat = await localMode.$('.thread .empty')
    await browser.waitUntil(async () => await emptyChat.getText() === 'Connect a model in the composer to start chatting.', {
      timeout: 30000,
      timeoutMsg: 'empty chat did not explain how to connect a model before the first connection',
    })
    if (!(await settings.isDisplayed())) {
      const modelChip = await localMode.$('.model-chip')
      await modelChip.waitForDisplayed()
      await modelChip.click()
      await (await $('button=Models & routing')).click()
      await settings.waitForDisplayed()
    }
    await (await settings.$('button=Connect account')).click()
    await (await settings.$('button*=Ollama')).click()
    const baseUrlInput = await $('#provider-base-url')
    await baseUrlInput.waitForDisplayed()
    await baseUrlInput.setValue(OLLAMA_BASE_URL)
    await (await $('button=Save Ollama server')).click()
    await (await $('p=Muniment saved the Ollama server.')).waitForDisplayed({ timeout: 30000 })
    await (await settings.$('button[aria-label="Close settings"]')).click()
    await settings.waitForDisplayed({ reverse: true, timeout: 10000 })

    await browser.waitUntil(async () => (await emptyChat.getText()).endsWith('is selected. Ask a question or request a file.'), {
      timeout: 30000,
      timeoutMsg: 'empty chat did not reflect the connected model or explain file creation',
    })
    await waitForDesktopClient()
    const prompt = `Muniment local E2E chat ${Date.now()}`
    await composer.setValue(prompt)
    const launchStarted = Date.now()
    await (await $('button[aria-label="Send"]')).click()

    const userMessage = await $(`//div[contains(concat(' ', normalize-space(@class), ' '), ' user-turn ')]//p[normalize-space()="${prompt}"]`)
    await userMessage.waitForDisplayed()
    const response = await userMessage.$('./ancestor::div[contains(concat(" ", normalize-space(@class), " "), " user-turn ")]/following-sibling::div[contains(concat(" ", normalize-space(@class), " "), " response ")][1]')
    await browser.waitUntil(async () => {
      const streaming = await response.$('.response-prose.streaming')
      if (await streaming.isDisplayed() && (await streaming.getText()).trim()) return true
      return await (await response.$('button.provenance')).isDisplayed()
    }, {
      timeout: 180000,
      timeoutMsg: `local-mode chat did not render its first reply for prompt: ${prompt}`,
    })
    console.log(`The first local-mode reply took ${((Date.now() - launchStarted) / 1000).toFixed(1)} seconds.`)
    await checkCandidatePackages()

    await browser.waitUntil(async () => !(await (await $('button=Stop')).isExisting()), { timeout: 180000 })
    const registered = await browser.execute(async () => window.__TAURI__.core.invoke(
      'plugin:global-shortcut|is_registered', { shortcut: 'Control+Alt+Space' },
    ))
    expect(registered).toBe(true)
    console.log('The launcher registered Control+Alt+Space.')
    const previousThread = await browser.execute(async () => window.__TAURI__.core.invoke('chat_current_thread'))
    const mainHandle = await browser.getWindowHandle()
    await browser.execute(async () => window.__TAURI__.core.invoke('launcher_open'))
    let launcherHandle
    for (const handle of await browser.getWindowHandles()) {
      await browser.switchToWindow(handle)
      if (await browser.execute(() => new URLSearchParams(location.search).has('launcher'))) {
        launcherHandle = handle
        break
      }
    }
    expect(launcherHandle).toBeTruthy()
    const launcher = await $('input[aria-label="First message"]')
    await launcher.waitForDisplayed()
    expect(await launcher.getAttribute('aria-invalid')).toBe('false')
    expect(await (await $('[role="alert"]')).getText()).toBe('')
    expect(await launcher.isFocused()).toBe(true)
    expect(await browser.execute(async () => window.__TAURI__.core.invoke('launcher_is_visible'))).toBe(true)
    await browser.keys('Escape')
    await browser.switchToWindow(mainHandle)
    expect(await browser.execute(async () => window.__TAURI__.core.invoke('launcher_is_visible'))).toBe(false)
    await browser.execute(async () => window.__TAURI__.core.invoke('launcher_open'))
    await browser.switchToWindow(launcherHandle)
    const launcherPrompt = `Launcher local E2E chat ${Date.now()}`
    await launcher.setValue(launcherPrompt)
    await browser.keys('Enter')
    await browser.switchToWindow(mainHandle)
    await browser.waitUntil(async () => (await (await $('.thread')).getText()).includes(launcherPrompt))
    await browser.waitUntil(async () => !(await browser.execute(async () => window.__TAURI__.core.invoke('launcher_is_visible'))))
    expect(await composer.getValue()).toBe('')
    const currentThread = await browser.execute(async () => window.__TAURI__.core.invoke('chat_current_thread'))
    expect(currentThread).toBeTruthy()
    expect(currentThread).not.toBe(previousThread)
    expect(await (await $('.thread')).getText()).not.toContain(prompt)
  }).timeout(300000)
})
