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
