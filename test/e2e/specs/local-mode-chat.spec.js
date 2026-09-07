import path from 'node:path'
import { access, readFile } from 'node:fs/promises'
import { OLLAMA_BASE_URL } from '../support/local-provider.mjs'
import { piAgentDirectory } from '../support/pi-agent-directory.mjs'

async function checkCandidatePackages() {
  if (process.env.MUNIMENT_PI_CANDIDATE !== '1') return
  const agentDirectory = piAgentDirectory()
  const packages = [
    ['pi-web-access', '0.28.0'],
    ['pi-subagents', '0.65.1'],
    ['pi-background-tasks', '2.5.0'],
    ['pi-mcp-adapter', '2.32.1'],
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
  const home = await location.getProperty('textContent')
  expect(path.isAbsolute(home)).toBe(true)
  let homeExists = true
  try { await access(home) } catch { homeExists = false }
  expect(homeExists).toBe(false)
  await (await $('[data-testid="onboarding-confirm"]')).click()
  const skipImport = await $('button=Continue without importing')
  await skipImport.waitForDisplayed()
  await skipImport.click()
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
    const localMode = await $('button=Use local mode')
    await localMode.waitForDisplayed({ timeout: 120000 })
    await localMode.click()

    const composer = await $('textarea[placeholder="Ask anything"]')
    await composer.waitForDisplayed({ timeout: 120000 })
    const provider = await $('input[name="provider"][value="ollama"]')
    await provider.click()
    const baseUrlInput = await $('#provider-base-url')
    await baseUrlInput.waitForDisplayed()
    expect(await baseUrlInput.isDisplayed()).toBe(true)
    await baseUrlInput.setValue(OLLAMA_BASE_URL)
    await (await $('button=Save Ollama server')).click()
    await (await $('p=Pi saved the Ollama server.')).waitForDisplayed({ timeout: 30000 })
    expect(await baseUrlInput.getValue()).toBe('')

    await waitForDesktopClient()
    const prompt = `Muniment local E2E chat ${Date.now()}`
    await composer.setValue(prompt)
    const launchStarted = Date.now()
    await (await $('button=Send')).click()

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
  }).timeout(300000)
})
