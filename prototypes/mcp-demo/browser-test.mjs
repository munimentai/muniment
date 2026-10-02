import assert from 'node:assert/strict'
import { mkdtemp, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFile } from 'node:child_process'
import { promisify } from 'node:util'
import { chromium } from 'playwright'
import { startDemo } from './server.mjs'

const evidence = await mkdtemp(join(tmpdir(), 'muniment-mcp-evidence-'))
const demo = await startDemo()
let browser, page
try {
  browser = await chromium.launch({ headless: true })
  page = await browser.newPage({ viewport: { width: 1440, height: 1100 } })
  page.setDefaultTimeout(15000)
  const errors = []
  page.on('pageerror', error => { errors.push(error.message); console.error(error.message) })
  const consent = []
  page.on('dialog', dialog => (consent.shift() ?? true) ? dialog.accept() : dialog.dismiss())
  await page.goto(demo.origin)
  await promisify(execFile)(process.execPath, [fileURLToPath(new URL('./cli.js', import.meta.resolve('playwright'))), 'screenshot', '--browser', 'chromium', '--viewport-size', '1440,1100', '--wait-for-timeout', '1000', demo.origin, join(evidence, 'launch.png')], { timeout: 30000 })
  const search = page.getByRole('searchbox', { name: 'Search MCP servers' })
  await search.fill('Notion')
  const catalog = page.getByRole('region', { name: 'Settings Extend' })
  assert.equal(await catalog.getByRole('button', { name: 'Connect', exact: true }).count(), 1)
  await catalog.getByRole('button', { name: 'Details', exact: true }).click()
  const details = page.getByRole('dialog', { name: 'Notion', exact: true })
  await details.getByText('Server URL', { exact: true }).waitFor()
  await details.getByRole('button', { name: 'Connect', exact: true }).waitFor()
  consent.push(true, false)
  await details.getByRole('button', { name: 'Connect', exact: true }).click()
  await catalog.getByRole('alert').filter({ hasText: 'The demo did not connect to the server.' }).waitFor()
  const provider = (await demo.boundary.dispatch('read')).items[0]
  assert.equal(provider.name, 'Notion')
  assert.equal(provider.definition.url, 'https://mcp.notion.com/mcp')
  assert.ok(provider.source)
  assert.equal(provider.lastCheck, undefined)
  await catalog.getByRole('button', { name: 'Details', exact: true }).click()
  await details.getByRole('button', { name: 'Uninstall', exact: true }).click()
  await catalog.getByRole('button', { name: 'Connect', exact: true }).waitFor()
  assert.equal((await demo.boundary.dispatch('read')).items.length, 0)
  await search.fill('')
  await page.screenshot({ path: join(evidence, 'catalog.png'), fullPage: true })

  async function add(name, config) {
    await page.getByRole('button', { name: 'Custom', exact: true }).click()
    const form = page.getByRole('dialog', { name: 'Add MCP server' })
    await form.getByLabel('Name', { exact: true }).fill(name)
    await form.getByText('Local command or advanced configuration', { exact: true }).click()
    await form.getByLabel('Server configuration').fill(JSON.stringify(config))
    if (name === 'Workshop') await page.screenshot({ path: join(evidence, 'custom.png') })
    await form.getByRole('button', { name: 'Save server', exact: true }).click()
    await form.waitFor({ state: 'hidden' })
  }
  await add('Workshop', demo.fixture.stdio)
  await page.reload()
  await page.getByLabel('Saved server', { exact: true }).selectOption({ label: 'Workshop' })
  await page.getByRole('button', { name: 'Connect and list tools' }).click()
  await page.getByLabel('Allow this tool call and fixture quote updates.').check()
  await page.getByRole('button', { name: 'Run candidate chat' }).click()
  await page.locator('.reply').filter({ hasText: '3 workshop seats cost $75.' }).waitFor()
  await page.getByRole('button', { name: 'Open comparison app' }).click()
  const app = page.frameLocator('iframe[title="Workshop MCP app"]')
  await app.getByText('3 seats at $25 each.', { exact: true }).waitFor()
  await app.getByLabel('Seats', { exact: true }).fill('5')
  await app.getByRole('button', { name: 'Update quote' }).click()
  await app.getByText('5 seats at $25 each.', { exact: true }).waitFor()
  assert.equal(await app.getByLabel('Total').textContent(), '$125')
  const isolated = await page.frames().find(frame => frame !== page.mainFrame()).evaluate(() => {
    try { return !!parent.document } catch { return false }
  })
  assert.equal(isolated, false)
  await search.fill('Workshop')
  await page.screenshot({ path: join(evidence, 'comparison.png'), fullPage: true })
  await page.getByLabel('Allow this tool call and fixture quote updates.').uncheck()
  await page.locator('iframe').waitFor({ state: 'detached' })
  assert.equal(await page.getByRole('button', { name: 'Run candidate chat' }).isDisabled(), true)
  await page.getByLabel('Allow this tool call and fixture quote updates.').check()
  await page.getByLabel('Candidate exposure').selectOption('direct')
  await page.getByRole('button', { name: 'Run candidate chat' }).click()
  await page.locator('.reply').filter({ hasText: '3 workshop seats cost $75.' }).waitFor()
  const { id, revision } = (await demo.boundary.dispatch('read')).items[0]
  const raw = await demo.boundary.dispatch('app', { id, revision, approved: true, args: { seats: 3 } })
  const candidate = await demo.boundary.dispatch('chat', { id, revision, approved: true, tool: 'quote', args: { seats: 3 }, exposure: 'direct' })
  await writeFile(join(evidence, 'boundary.json'), `${JSON.stringify({ tool: raw.tool, rawResult: raw.result, candidate }, null, 2)}\n`)

  const second = await browser.newPage()
  second.setDefaultTimeout(15000)
  second.on('pageerror', error => errors.push(error.message))
  second.on('dialog', dialog => dialog.accept())
  await second.goto(demo.origin)
  await second.getByRole('searchbox', { name: 'Search MCP servers' }).fill('Workshop')
  async function replaceInSecondTab(definition) {
    await second.getByRole('region', { name: 'Settings Extend' }).getByRole('button', { name: 'Details', exact: true }).click()
    await second.getByRole('dialog', { name: 'Workshop', exact: true }).getByRole('button', { name: 'Configure', exact: true }).click()
    const editor = second.getByRole('dialog', { name: 'Add MCP server' })
    await editor.getByText('Local command or advanced configuration', { exact: true }).click()
    await editor.getByLabel('Server configuration').fill(JSON.stringify(definition))
    await editor.getByRole('button', { name: 'Save server', exact: true }).click()
    await editor.waitFor({ state: 'hidden' })
    await second.getByRole('button', { name: 'Connect and list tools' }).click()
    await second.getByLabel('Allow this tool call and fixture quote updates.').waitFor({ state: 'visible' })
    await second.waitForFunction(() => !document.querySelector('.approval input').disabled)
  }
  for (const action of ['chat', 'app', 'app-call']) {
    if (action === 'app-call') {
      await page.getByRole('button', { name: 'Open comparison app' }).click()
      await app.getByText('3 seats at $25 each.', { exact: true }).waitFor()
    }
    await replaceInSecondTab(demo.fixture.http)
    assert.equal(await page.getByLabel('Allow this tool call and fixture quote updates.').isChecked(), true)
    const rejected = page.waitForResponse(async response => response.url().endsWith('/api/command') && response.request().postDataJSON().action === action)
    if (action === 'app-call') {
      await app.getByLabel('Seats', { exact: true }).fill('6')
      await app.getByRole('button', { name: 'Update quote' }).click()
    } else {
      await page.getByRole('button', { name: action === 'chat' ? 'Run candidate chat' : 'Open comparison app' }).click()
    }
    assert.equal((await rejected).status(), 400)
    await page.getByRole('alert').filter({ hasText: 'The server changed. Connect again and approve a fresh tool call.' }).waitFor()
    await page.locator('iframe').waitFor({ state: 'detached' })
    assert.equal(await page.getByLabel('Allow this tool call and fixture quote updates.').isChecked(), false)
    assert.equal(await page.getByRole('button', { name: 'Run candidate chat' }).isDisabled(), true)
    await page.getByRole('button', { name: 'Connect and list tools' }).click()
    await page.getByLabel('Allow this tool call and fixture quote updates.').check()
    await page.getByRole('button', { name: 'Run candidate chat' }).click()
    await page.locator('.reply').filter({ hasText: '3 workshop seats cost $75.' }).waitFor()
    await replaceInSecondTab(demo.fixture.stdio)
    await page.getByRole('button', { name: 'Connect and list tools' }).click()
    await page.getByLabel('Allow this tool call and fixture quote updates.').check()
  }
  await second.close()

  await catalog.getByRole('button', { name: 'Details', exact: true }).click()
  const savedDetails = page.getByRole('dialog', { name: 'Workshop', exact: true })
  await savedDetails.getByRole('button', { name: 'Configure', exact: true }).click()
  const form = page.getByRole('dialog', { name: 'Add MCP server' })
  await form.getByText('Local command or advanced configuration', { exact: true }).click()
  await form.getByLabel('Server configuration').fill(JSON.stringify({ command: 'muniment-demo-command-does-not-exist' }))
  await form.getByRole('button', { name: 'Save server', exact: true }).click()
  await form.waitFor({ state: 'hidden' })
  await catalog.getByRole('button', { name: 'Details', exact: true }).click()
  await savedDetails.getByRole('button', { name: 'Test connection' }).click()
  await savedDetails.getByRole('alert').filter({ hasText: 'Connection failed.' }).waitFor()
  await page.screenshot({ path: join(evidence, 'connection-error.png') })
  await savedDetails.getByRole('button', { name: 'Configure', exact: true }).click()
  await form.getByText('Local command or advanced configuration', { exact: true }).click()
  await form.getByLabel('Server configuration').fill(JSON.stringify(demo.fixture.stdio))
  await form.getByRole('button', { name: 'Save server', exact: true }).click()
  await form.waitFor({ state: 'hidden' })
  await page.getByRole('button', { name: 'Connect and list tools' }).click()
  await page.getByLabel('Allow this tool call and fixture quote updates.').check()
  await catalog.getByRole('switch', { name: 'Use Workshop in chats' }).click()
  assert.equal(await catalog.getByRole('switch', { name: 'Use Workshop in chats' }).getAttribute('aria-checked'), 'false')
  await catalog.getByRole('switch', { name: 'Use Workshop in chats' }).click()
  await catalog.getByRole('button', { name: 'Details', exact: true }).click()
  await savedDetails.getByRole('button', { name: 'Uninstall' }).click()
  await page.getByText('No servers match these filters.').waitFor()
  assert.equal((await demo.boundary.dispatch('read')).items.length, 0)
  await page.getByRole('button', { name: 'Custom', exact: true }).click()
  await form.getByLabel('Name', { exact: true }).fill('HTTP workshop')
  await form.getByLabel('Server URL', { exact: true }).fill(demo.fixture.http.url)
  await form.locator('select').selectOption('oauth')
  await form.getByRole('button', { name: 'Save and sign in' }).click()
  await form.waitFor({ state: 'hidden' })
  await page.getByRole('status').filter({ hasText: 'HTTP workshop: connected.' }).waitFor()
  await page.getByRole('button', { name: 'Connect and list tools' }).click()
  await page.getByLabel('Allow this tool call and fixture quote updates.').check()
  await page.getByRole('button', { name: 'Run candidate chat' }).click()
  await page.locator('.reply').filter({ hasText: '3 workshop seats cost $75.' }).waitFor()
  assert.deepEqual(errors, [])
  console.log(`The browser checks passed. Evidence: ${evidence}`)
} catch (error) {
  await page?.screenshot({ path: join(evidence, 'failure.png'), fullPage: true })
  console.error(`The browser check failed. Evidence: ${evidence}`)
  throw error
} finally {
  await browser?.close()
  await demo.close()
}
