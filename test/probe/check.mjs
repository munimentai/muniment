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
            // On macOS the native clearance is the sidebar part's padding, measured from the window edge.
            clearance: row.querySelector('.titlebar-sidebar') ? row.querySelector('.titlebar-sidebar').getBoundingClientRect().left + parseFloat(getComputedStyle(row.querySelector('.titlebar-sidebar')).paddingLeft) : null,
            controls: [...row.querySelectorAll('button, input')].map((control) => {
              const box = control.getBoundingClientRect()
              const target = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)
              return {
                name: control.getAttribute('aria-label'),
                width: box.width,
                height: box.height,
                // The row ends where its controls end, so the paper under them is the frame.
                flush: Math.abs(box.bottom - rect.bottom) < 0.5,
                visible: control.contains(target),
                inside: box.left >= rect.left && box.right <= rect.right && box.top >= rect.top && box.bottom <= rect.bottom,
                draggable: control.hasAttribute('data-tauri-drag-region'),
              }
            }),
          }
        })
        // The title row is 30px on the 36px band the workspace grid names.
        assert.equal(row.height, 30)
        assert.equal(row.top, 0)
        if (platform.startsWith('Mac')) assert.equal(row.clearance, 78)
        else assert.equal(row.padding, '12px')
        assert.equal(row.controls.length, 4)
        for (const control of row.controls) {
          assert.ok(control.width >= 24 && control.height >= 24, JSON.stringify(control))
          assert.ok(control.visible && control.inside && control.flush, JSON.stringify(control))
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
      assert.equal(await page.locator('.new-thread').count(), 0)
      await page.getByRole('button', { name: 'Expand sidebar', exact: true }).click()
      await checkRow()
      assert.equal(await page.locator('.new-thread').innerText(), platform.startsWith('Mac') ? 'New thread\n⌘ N' : 'New thread\nCtrl N')
      await page.getByRole('button', { name: 'Artifacts', exact: true }).click()
      await checkArtifactCreation(page)
      await checkRow()
      await page.getByRole('button', { name: 'Close browser panel', exact: true }).click()
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

async function checkArtifactCreation(page) {
  const panel = page.getByRole('region', { name: 'Artifacts', exact: true })
  assert.equal(await panel.getByRole('heading', { name: 'Create an Artifact' }).count(), 1)
  assert.equal(await panel.getByRole('textbox', { name: 'Name', exact: true }).count(), 1)
  assert.equal(await panel.getByRole('textbox', { name: 'Artifact HTML' }).count(), 1)
  assert.equal(await panel.getByRole('button', { name: 'Save and preview' }).isEnabled(), true)
}

async function checkPaperFrame(browser, baseUrl) {
  for (const platform of ['MacIntel', 'Win32', 'Linux x86_64']) {
    for (const colorScheme of ['light', 'dark']) {
      const page = await browser.newPage({ colorScheme })
      try {
        await page.addInitScript((platform) => {
          Object.defineProperty(navigator, 'platform', { get: () => platform })
        }, platform)
        await page.goto(`${baseUrl}/test/probe/history.html`)
        await page.waitForSelector('[data-probe-ready]')
        await page.evaluate(() => document.fonts.ready)

        const checkFrame = async (transition = false) => {
          const failures = await page.evaluate(async (transition) => {
            const failures = []
            const check = () => {
              const workspace = document.querySelector('.workspace')
              const style = getComputedStyle(workspace)
              const frame = parseFloat(style.paddingRight)
              const color = (token) => {
                const sample = document.createElement('span')
                sample.style.color = style.getPropertyValue(token)
                workspace.append(sample)
                const value = getComputedStyle(sample).color
                sample.remove()
                return value
              }
              const paper = color('--paper')
              const surface = color('--surface')
              const border = color('--border')
              // A collapsed sidebar is gone: the panels are the thread and the rail, and the thread slides to the frame.
              const collapsed = workspace.classList.contains('sidebar-collapsed')
              const panels = [...workspace.querySelectorAll(collapsed ? '.thread-panel, .record-panel' : '.sidebar, .thread-panel, .record-panel')]
              const boxes = panels.map((panel) => panel.getBoundingClientRect())
              const threadBox = workspace.querySelector('.thread-panel').getBoundingClientRect()
              const fail = (condition, message) => { if (!condition) failures.push(message) }
              const near = (left, right) => Math.abs(left - right) < 1
              const sliding = transition && (collapsed || parseFloat(getComputedStyle(workspace.querySelector('.thread-panel')).marginLeft) !== 0)
              fail(frame === 8, `Frame width: ${frame}`)
              fail(style.backgroundColor === paper, 'The workspace lacks paper.')
              fail(sliding ? boxes[0].left >= frame - 0.5 : near(boxes[0].left, frame), 'The left frame changed.')
              fail(near(boxes.at(-1).right, innerWidth - frame), 'The right frame changed.')
              for (const [index, panel] of panels.entries()) {
                const box = boxes[index]
                const panelStyle = getComputedStyle(panel)
                // The panels start at the 36px band the workspace grid names, not at the title row plus the frame.
                fail(near(box.top, parseFloat(style.getPropertyValue('--titlebar-band'))), 'The top frame changed.')
                fail(near(box.bottom, innerHeight - frame), 'The bottom frame changed.')
                fail(panelStyle.backgroundColor === surface, `${panel.className} lacks surface.`)
                for (const edge of ['Top', 'Right', 'Bottom', 'Left']) {
                  fail(panelStyle[`border${edge}Width`] === '1px', `${panel.className} lacks a hairline.`)
                  fail(panelStyle[`border${edge}Color`] === border, `${panel.className} has the wrong border color.`)
                }
                for (const corner of ['TopLeft', 'TopRight', 'BottomLeft', 'BottomRight']) {
                  fail(panelStyle[`border${corner}Radius`] === style.getPropertyValue('--radius-panel').trim(), `${panel.className} has the wrong radius.`)
                }
                if (index) fail(index === 1 && sliding ? box.left - boxes[0].right <= frame + 0.5 : near(box.left - boxes[index - 1].right, frame), 'The panel gap changed.')
              }
              fail(threadBox.width >= 319.9, 'The thread fell below 320px.')
              const composer = workspace.querySelector('.composer').getBoundingClientRect()
              fail(composer.left > threadBox.left && composer.right < threadBox.right && composer.bottom < threadBox.bottom, 'The composer escaped the thread panel.')
              if (workspace.classList.contains('macos')) {
                const rect = (selector) => workspace.querySelector(selector).getBoundingClientRect()
                const record = rect('.record-toggle')
                const update = rect('.update-slot')
                fail(near(record.right, innerWidth - 12), 'Record does not sit flush right in the title row.')
                fail(update.right <= record.left, 'The update slot extends past Record.')
                const controlsEnd = parseFloat(style.getPropertyValue('--titlebar-controls-end'))
                const titleLeft = Math.max(threadBox.left - parseFloat(getComputedStyle(workspace.querySelector('.thread-panel')).marginLeft), controlsEnd)
                fail(near(rect('.thread-title').left, titleLeft), `The thread title starts at ${rect('.thread-title').left}, expected ${titleLeft}; collapsed=${collapsed}.`)
                fail(workspace.querySelector('.titlebar .new-thread') === null, 'New thread must stay in the sidebar.')
                if (!collapsed) fail(workspace.querySelector('.sidebar .new-thread') !== null, 'The sidebar lacks New thread.')
                for (const control of workspace.querySelectorAll('.titlebar button, .titlebar input')) {
                  const box = control.getBoundingClientRect()
                  const target = document.elementFromPoint(box.x + box.width / 2, box.y + box.height / 2)
                  fail(box.width >= 24 && box.height >= 24 && control.contains(target), 'A title row control lacks a hit area.')
                }
              }
              const backgroundAt = (x, y) => {
                let element = document.elementFromPoint(x, y)
                while (element) {
                  const color = getComputedStyle(element).backgroundColor
                  if (color !== 'rgba(0, 0, 0, 0)') return color
                  element = element.parentElement
                }
                return null
              }
              for (const [x, y] of [[innerWidth / 2, 1], [1, innerHeight / 2], [innerWidth - 1, innerHeight / 2], [innerWidth / 2, innerHeight - 1]]) {
                fail(backgroundAt(x, y) === paper, 'A window edge lacks paper.')
              }
              for (let index = sliding ? 2 : 1; index < boxes.length; index++) {
                for (let y = boxes[index].top + 1; y < boxes[index].bottom; y += 4) {
                  fail(backgroundAt(boxes[index - 1].right + 1, y) === paper, 'A panel gap lacks paper.')
                }
              }
            }
            const start = performance.now()
            do {
              await new Promise(requestAnimationFrame)
              check()
            } while (transition && performance.now() - start < 240)
            return [...new Set(failures)]
          }, transition)
          assert.deepEqual(failures, [], `${platform} ${colorScheme} ${JSON.stringify(page.viewportSize())}`)
          if (platform.startsWith('Mac')) {
            const aligned = await page.evaluate(() => {
              const workspace = document.querySelector('.workspace')
              if (workspace.classList.contains('sidebar-collapsed')) return true
              // The title follows the thread panel once the sidebar is wider than the row's own controls.
              const title = workspace.querySelector('.thread-title').getBoundingClientRect()
              const panel = workspace.querySelector('.thread-panel').getBoundingClientRect()
              const controlsEnd = parseFloat(getComputedStyle(workspace).getPropertyValue('--titlebar-controls-end'))
              return Math.abs(title.left - Math.max(panel.left, controlsEnd)) < 1
            })
            assert.ok(aligned, 'The expanded thread title does not align with the thread panel or the row controls.')
          }
        }

        for (const viewport of [{ width: 960, height: 640 }, { width: 1144, height: 640 }, { width: 1440, height: 900 }]) {
          await page.setViewportSize(viewport)
          await checkFrame()
          await page.getByRole('button', { name: 'Open record panel', exact: true }).click()
          await checkFrame(true)
          const divider = page.getByRole('separator', { name: 'Record', exact: true })
          await divider.press('End')
          await checkFrame(true)
          await page.getByRole('button', { name: 'Collapse sidebar', exact: true }).click()
          await checkFrame(true)
          await divider.press('End')
          await checkFrame(true)
          await page.getByRole('button', { name: 'Expand sidebar', exact: true }).click()
          await checkFrame(true)
          const box = await divider.boundingBox()
          await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)
          await page.mouse.down()
          await page.mouse.move(viewport.width - 408, box.y + box.height / 2)
          await page.mouse.up()
          await checkFrame(true)
          await page.getByRole('button', { name: 'Close record panel', exact: true }).click()
          await checkFrame(true)
          await page.emulateMedia({ reducedMotion: 'reduce' })
          await page.getByRole('button', { name: 'Collapse sidebar', exact: true }).click()
          await page.getByRole('button', { name: 'Open record panel', exact: true }).click()
          await checkFrame()
          await divider.press('Escape')
          await page.getByRole('button', { name: 'Expand sidebar', exact: true }).click()
          await checkFrame()
          await page.emulateMedia({ reducedMotion: 'no-preference' })
        }
        console.log(`Paper frame checks passed for ${platform} in ${colorScheme}.`)
      } finally {
        await page.close()
      }
    }
  }
}

async function checkComposerActions(browser, baseUrl) {
  for (const fixture of ['index.html', 'local-mode.html', 'in-flight.html']) {
    const page = await browser.newPage({ viewport: { width: 960, height: 640 } })
    try {
      await page.goto(`${baseUrl}/test/probe/${fixture}`)
      await page.waitForSelector('[data-probe-ready]')
      await page.evaluate(() => document.fonts.ready)
      for (const rail of ['closed', 'open', 'maximum']) {
        if (rail === 'open') await page.getByRole('button', { name: 'Open record panel', exact: true }).click()
        for (const width of [960, 1100, 1101, 1280, 1440, 1920, 960]) {
          await page.setViewportSize({ width, height: 640 })
          if (rail === 'maximum') {
            const divider = page.getByRole('separator', { name: 'Record', exact: true })
            await divider.press('End')
          }
          await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))
          const layout = await page.locator('.composer-row').evaluate((row) => {
            const box = row.getBoundingClientRect()
            // Local mode carries no hint: the row holds the actions alone.
            const hint = row.querySelector('#composer-hint')
            const hintBox = hint?.getBoundingClientRect()
            const actions = row.querySelector('.composer-actions')
            const actionsBox = actions.getBoundingClientRect()
            const range = document.createRange()
            if (hint) range.selectNodeContents(hint)
            return {
              width: box.width,
              hintWidth: hint ? range.getBoundingClientRect().width : 0,
              hintLines: hint ? range.getClientRects().length : 0,
              actionsWidth: actionsBox.width,
              gap: parseFloat(getComputedStyle(row).columnGap) || 0,
              separateRows: !hint || hintBox.bottom <= actionsBox.top,
              buttons: [...actions.querySelectorAll('button')].map((button) => {
                const rect = button.getBoundingClientRect()
                range.selectNodeContents(button)
                const textRects = [...range.getClientRects()]
                return {
                  label: button.getAttribute('aria-label') ?? button.textContent,
                  lines: new Set(textRects.map((rect) => rect.top)).size,
                  width: rect.width,
                  height: rect.height,
                  inside: rect.left >= box.left && rect.right <= box.right && rect.bottom <= window.innerHeight,
                  textInside: textRects.every((text) => text.left >= rect.left && text.right <= rect.right),
                }
              }),
            }
          })
          const context = JSON.stringify({ fixture, rail, width, layout })
          // Add files leads the row at its left; the actions hold the microphone, and a stop square in flight.
          assert.deepEqual(layout.buttons.map(({ label }) => label), fixture === 'in-flight.html'
            ? ['Voice', 'Stop']
            : ['Voice'], context)
          for (const button of layout.buttons) {
            assert.equal(button.lines, 1, context)
            assert.ok(button.width >= 24 && button.height >= 24, context)
            assert.ok(button.inside && button.textInside, context)
          }
          if (layout.hintLines > 1 || layout.hintWidth + layout.actionsWidth + layout.gap > layout.width) {
            assert.ok(layout.separateRows, context)
          }
        }
      }
      console.log(`Composer action checks passed for ${fixture}.`)
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
    await checkPaperFrame(browser, baseUrl)
    await checkComposerActions(browser, baseUrl)
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
