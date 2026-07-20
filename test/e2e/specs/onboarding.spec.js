import path from 'node:path'
import { access, readFile } from 'node:fs/promises'

describe('installed nightly model-ready onboarding', () => {
  it('requires location consent, confirms a model report, and scaffolds every seed', async () => {
    const home = process.env.MUNIMENT_E2E_HOME_PATH
    const location = await $('[data-testid="onboarding-home-path"]')
    await location.waitForDisplayed()
    expect(await location.getValue()).toMatch(/[\\/]Documents[\\/]Muniment$/)
    expect(await $('[data-testid="onboarding-review"]').isEnabled()).toBe(false)

    await location.setValue(home)
    await (await $('[data-testid="onboarding-accept-location"]')).click()
    await (await $('[data-testid="onboarding-review"]')).click()
    const report = await $('[data-testid="onboarding-report"]')
    await report.waitForDisplayed()
    expect(await report.getText()).toContain('On-device triage')
    let homeExists = true
    try { await access(home) } catch { homeExists = false }
    expect(homeExists).toBe(false)
    await (await $('[data-testid="onboarding-confirm"]')).click()
    await (await $('button=Sign in')).waitForDisplayed()

    for (const directory of ['memory', 'agents', 'projects', 'sessions']) {
      expect(await readFile(path.join(home, directory, 'README.md'), 'utf8')).toContain(`# ${directory[0].toUpperCase()}${directory.slice(1)}`)
    }
    await access(path.join(home, 'agents', 'researcher.md'))
    await access(path.join(home, 'agents', 'writer.md'))
  })
})
