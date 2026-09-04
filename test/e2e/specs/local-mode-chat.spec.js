import path from 'node:path'
import { access } from 'node:fs/promises'

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
    const providerKey = process.env.MUNIMENT_E2E_PROVIDER_KEY
    if (!providerKey) throw new Error('the E2E provider credential is unavailable')

    await completeOnboarding()
    const localMode = await $('button=Use local mode')
    await localMode.waitForDisplayed({ timeout: 120000 })
    await localMode.click()

    const composer = await $('textarea[placeholder="Ask anything"]')
    await composer.waitForDisplayed({ timeout: 120000 })
    const keyInput = await $('#provider-key')
    await keyInput.waitForDisplayed()
    await keyInput.setValue(providerKey)
    await (await $('button=Save Google key')).click()
    await (await $('p=Pi saved the provider key.')).waitForDisplayed({ timeout: 30000 })
    expect(await keyInput.getValue()).toBe('')

    await waitForDesktopClient()
    const prompt = `Muniment local E2E chat ${Date.now()}`
    await composer.setValue(prompt)
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
  }).timeout(300000)
})
