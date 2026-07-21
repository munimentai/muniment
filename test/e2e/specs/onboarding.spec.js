import path from 'node:path'
import { access, readFile } from 'node:fs/promises'

describe('installed nightly model-ready onboarding', () => {
  it('chooses an isolated Home and scaffolds its README files', async () => {
    const home = process.env.MUNIMENT_E2E_HOME_PATH
    const location = await $('[data-testid="onboarding-home-path"]')
    await location.waitForDisplayed()

    const dialog = await browser.tauri.mock('plugin:dialog|open')
    await dialog.mockReturnValue(home)
    await (await $('[data-testid="onboarding-picker"]')).click()
    expect(await location.getText()).toBe(home)
    let homeExists = true
    try { await access(home) } catch { homeExists = false }
    expect(homeExists).toBe(false)
    await (await $('[data-testid="onboarding-confirm"]')).click()
    await (await $('button=Sign in')).waitForDisplayed()

    for (const directory of ['memory', 'agents', 'projects', 'sessions']) {
      expect(await readFile(path.join(home, directory, 'README.md'), 'utf8')).toContain(`# ${directory[0].toUpperCase()}${directory.slice(1)}`)
    }
  })
})
