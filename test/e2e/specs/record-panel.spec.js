import { expandSidebar } from '../support/first-run.mjs'

// The Record control sits flush right of Artifacts. It opens the record panel
// beside the thread, the first company is created from the panel, and the
// kind list reads the catalogue from the runtime.
describe('installed record panel', () => {
  afterEach(async () => {
    try {
      await browser.execute(async () => window.__TAURI__.core.invoke('local_mode_leave'))
    } catch (error) {
      console.error('Local mode cleanup failed.', error)
    }
  })

  it('opens beside the thread and lists the kinds of the first company', async () => {
    const localMode = await $('[data-testid="local-mode"]')
    await localMode.waitForDisplayed({ timeout: 120000 })
    await expandSidebar()

    const record = await $('button.record-toggle')
    await record.waitForDisplayed()
    const artifacts = await $('button.artifacts-toggle')
    const recordBox = await record.getLocation()
    const artifactsBox = await artifacts.getLocation()
    expect(recordBox.x).toBeGreaterThan(artifactsBox.x)

    await record.click()
    const panel = await $('[data-testid="record-panel"]')
    await panel.waitForDisplayed({ timeout: 30000 })
    expect(await record.getAttribute('aria-expanded')).toBe('true')

    const createButton = await panel.$('button=Create your company')
    if (await createButton.isExisting()) {
      await (await panel.$('input[aria-label="Company name"]')).setValue(`E2E ${Date.now()}`)
      await createButton.click()
    }
    const kinds = await panel.$('nav[aria-label="Kinds"]')
    await kinds.waitForDisplayed({ timeout: 60000 })
    const names = await Promise.all((await kinds.$$('.record-kind-name')).map((node) => node.getText()))
    expect(names.length).toBeGreaterThanOrEqual(20)
    expect(names).toContain('person')
    expect(names).toContain('deal')
    expect(await (await panel.$('select[aria-label="Company"]')).isDisplayed()).toBe(true)

    // A kind opens as a generated table; org has no records yet, so its one line says so.
    await (await kinds.$('.record-kind*=org')).click()
    const search = await panel.$('input[aria-label="Search org"]')
    await search.waitForDisplayed({ timeout: 30000 })
    await (await panel.$('p=No org records yet')).waitForDisplayed({ timeout: 30000 })
    await (await panel.$('button[aria-label="Back"]')).click()
    await kinds.waitForDisplayed({ timeout: 10000 })

    await browser.keys('Escape')
    await panel.waitForDisplayed({ reverse: true, timeout: 10000 })
    expect(await record.getAttribute('aria-expanded')).toBe('false')
  })
})
