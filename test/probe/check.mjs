import assert from 'node:assert/strict'
import { readFile, readdir, stat } from 'node:fs/promises'
import http from 'node:http'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'

const repositoryRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const probeDirectory = path.join(repositoryRoot, 'test/probe')
const contentTypes = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
}

function serveRepository() {
  const server = http.createServer(async (request, response) => {
    try {
      const pathname = decodeURIComponent(new URL(request.url, 'http://127.0.0.1').pathname)
      const relativePath = pathname.replace(/^\/+/, '')
      const filePath = path.resolve(repositoryRoot, relativePath)
      const relativeToRoot = path.relative(repositoryRoot, filePath)
      if (relativeToRoot.startsWith('..') || path.isAbsolute(relativeToRoot)) {
        response.writeHead(403).end('Forbidden')
        return
      }

      const fileStat = await stat(filePath)
      if (!fileStat.isFile()) throw new Error('Not a file')
      const body = await readFile(filePath)
      response.writeHead(200, {
        'Content-Type': contentTypes[path.extname(filePath)] ?? 'application/octet-stream',
      }).end(body)
    } catch {
      response.writeHead(404).end('Not found')
    }
  })

  return new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', () => resolve(server))
  })
}

async function checkFixture(browser, baseUrl, fixture) {
  const page = await browser.newPage({ viewport: { width: 1100, height: 720 } })
  const errors = []
  page.on('pageerror', (error) => errors.push(`page error: ${error.message}`))
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(`console error: ${message.text()}`)
  })

  try {
    await page.goto(`${baseUrl}/test/probe/${fixture}`, { waitUntil: 'load' })
    await page.waitForSelector('[data-probe-ready]', { timeout: 20_000 })
  } catch (error) {
    errors.push(`ready check: ${error.message}`)
  }

  try {
    const unknownCommands = await page.evaluate(() => window.__PROBE__?.unknownCommands ?? [])
    for (const command of unknownCommands) errors.push(`unknown command: ${command}`)
  } catch (error) {
    errors.push(`unknown-command check: ${error.message}`)
  }

  await page.close()
  return errors
}

async function checkWindowChrome(browser, baseUrl) {
  for (const platform of ['MacIntel', 'Win32', 'Linux x86_64']) {
    const page = await browser.newPage({ viewport: { width: 960, height: 640 } })
    try {
      await page.addInitScript((platform) => {
        Object.defineProperty(navigator, 'platform', { get: () => platform })
      }, platform)
      await page.goto(`${baseUrl}/test/probe/history.html`)
      await page.waitForSelector('[data-probe-ready]')
      const checkRow = async () => {
        const row = await page.locator('.titlebar').evaluate((row) => {
          const rect = row.getBoundingClientRect()
          return {
            height: rect.height,
            top: rect.top,
            padding: getComputedStyle(row).paddingLeft,
            controls: [...row.querySelectorAll('button, input')].map((control) => {
              const box = control.getBoundingClientRect()
              const target = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)
              return {
                name: control.getAttribute('aria-label'),
                width: box.width,
                height: box.height,
                visible: control.contains(target),
                inside: box.left >= rect.left && box.right <= rect.right && box.top >= rect.top && box.bottom <= rect.bottom,
                draggable: control.hasAttribute('data-tauri-drag-region'),
              }
            }),
          }
        })
        assert.equal(row.height, 36)
        assert.equal(row.top, 0)
        assert.equal(row.padding, platform.startsWith('Mac') ? '84px' : '12px')
        assert.equal(row.controls.length, 4)
        for (const control of row.controls) {
          assert.ok(control.width >= 24 && control.height >= 24, JSON.stringify(control))
          assert.ok(control.visible && control.inside, JSON.stringify(control))
          assert.equal(control.draggable, false)
        }
      }
      await checkRow()
      await page.getByRole('button', { name: 'Rename thread', exact: true }).click()
      await checkRow()
      await page.getByRole('textbox', { name: 'Thread name', exact: true }).fill('W'.repeat(160))
      await page.getByRole('textbox', { name: 'Thread name', exact: true }).press('Enter')
      await checkRow()
      await page.getByRole('button', { name: 'Collapse sidebar', exact: true }).click()
      await checkRow()
      assert.equal(await page.locator('.new-thread').innerText(), platform.startsWith('Mac') ? 'New thread\n⌘N' : 'New thread\nCtrl N')
      await page.getByRole('button', { name: 'Open artifact rail', exact: true }).click()
      await checkRow()
      await page.getByRole('button', { name: 'New thread', exact: true }).click()
      await page.waitForFunction(() => document.querySelector('button.thread-title')?.textContent === 'New thread')
      await checkRow()
      assert.deepEqual(await page.evaluate(() => window.__PROBE__.unknownCommands), [])
      await page.goto(`${baseUrl}/test/probe/index.html`)
      await page.waitForSelector('[data-probe-ready]')
      await checkRow()
      assert.equal(await page.getByRole('button', { name: 'Rename thread', exact: true }).isDisabled(), true)
      assert.deepEqual(await page.evaluate(() => window.__PROBE__.unknownCommands), [])
      console.log(`Window chrome checks passed for ${platform}.`)
    } finally {
      await page.close()
    }
  }
}

async function main() {
  const fixtures = (await readdir(probeDirectory))
    .filter((entry) => entry.endsWith('.html'))
    .sort()
  const server = await serveRepository()
  const address = server.address()
  const baseUrl = `http://127.0.0.1:${address.port}`
  let browser
  let failed = false

  try {
    browser = await chromium.launch({ headless: true })
    await checkWindowChrome(browser, baseUrl)
    for (const fixture of fixtures) {
      const errors = await checkFixture(browser, baseUrl, fixture)
      if (errors.length === 0) {
        console.log(`${fixture}: passed`)
        continue
      }
      failed = true
      for (const error of errors) console.error(`${fixture}: ${error}`)
    }
  } finally {
    await browser?.close()
    await new Promise((resolve, reject) => server.close((error) => error ? reject(error) : resolve()))
  }

  if (failed) process.exitCode = 1
}

main().catch((error) => {
  console.error(error)
  process.exitCode = 1
})
