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
      const relativePath = pathname.startsWith('/assets/') ? `dist${pathname}` : pathname.replace(/^\/+/, '')
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
                // The controls center in the row, which fills the band above the one-pixel frame.
                flush: Math.abs((box.top + box.bottom) / 2 - (rect.top + rect.bottom) / 2) < 0.5,
                visible: control.contains(target),
                inside: box.left >= rect.left && box.right <= rect.right && box.top >= rect.top && box.bottom <= rect.bottom,
                draggable: control.hasAttribute('data-tauri-drag-region'),
              }
            }),
          }
        })
        // The title row is 35px on the 36px band the workspace grid names, above the one-pixel frame.
        assert.equal(row.height, 35)
        assert.equal(row.top, 0)
        if (platform.startsWith('Mac')) assert.equal(row.clearance, 78)
        else assert.equal(row.padding, '12px')
        assert.equal(row.controls.length, 5)
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
  await panel.getByRole('button', { name: 'New artifact', exact: true }).click()
  // The composer shows the template as a chip and keeps its own text empty.
  const template = page.getByRole('group', { name: 'Template', exact: true })
  await template.waitFor()
  assert.match(await template.innerText(), /artifact/i)
  assert.equal(await page.locator('#composer-message').inputValue(), '')
  assert.equal(await page.evaluate(() => window.__PROBE__.invokedCommands.filter(item => item.command === 'creation_save').length), 0)
  await template.getByRole('button', { name: 'Cancel creation', exact: true }).click()
  assert.equal(await template.count(), 0)
  assert.equal(await page.locator('#composer-message').inputValue(), '')
}

// Both frames share one geometry: a one-pixel frame and gaps, so switching moves no content.
// Framed: a paper frame, a sidebar with no panel, and content panels drawn as cards clipped inside their boxes.
// Frameless: border seams, a flat sidebar panel, and flat surface panels.
async function checkPaperFrame(browser, baseUrl) {
  for (const frameName of ['framed', 'frameless']) {
  for (const platform of ['MacIntel', 'Win32', 'Linux x86_64']) {
    for (const colorScheme of ['light', 'dark']) {
      const page = await browser.newPage({ colorScheme })
      try {
        await page.addInitScript(([platform, frameName]) => {
          Object.defineProperty(navigator, 'platform', { get: () => platform })
          localStorage.setItem('muniment.frame', frameName)
        }, [platform, frameName])
        await page.goto(`${baseUrl}/test/probe/history.html`)
        await page.waitForSelector('[data-probe-ready]')
        await page.evaluate(() => document.fonts.ready)

        const checkFrame = async (transition = false) => {
          const failures = await page.evaluate(async ([transition, frameName]) => {
            const framed = frameName === 'framed'
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
              const sidebarPanel = color('--frame-sidebar-panel')
              const chrome = framed ? paper : border
              // A collapsed sidebar is gone: the panels are the thread and the rail, and the thread slides to the frame.
              const collapsed = workspace.classList.contains('sidebar-collapsed')
              const panels = [...workspace.querySelectorAll(collapsed ? '.thread-panel, .record-panel' : '.sidebar, .thread-panel, .record-panel')]
              const boxes = panels.map((panel) => panel.getBoundingClientRect())
              const threadBox = workspace.querySelector('.thread-panel').getBoundingClientRect()
              const fail = (condition, message) => { if (!condition) failures.push(message) }
              const near = (left, right) => Math.abs(left - right) < 1
              const sliding = transition && (collapsed || parseFloat(getComputedStyle(workspace.querySelector('.thread-panel')).marginLeft) !== 0)
              fail(document.documentElement.dataset.frame === frameName, `The root carries ${document.documentElement.dataset.frame}.`)
              fail(frame === 1, `Frame width: ${frame}`)
              fail(style.backgroundColor === chrome, 'The workspace lacks its frame color.')
              fail(sliding ? boxes[0].left >= frame - 0.5 : near(boxes[0].left, frame), 'The left frame changed.')
              fail(near(boxes.at(-1).right, innerWidth - frame), 'The right frame changed.')
              for (const [index, panel] of panels.entries()) {
                const box = boxes[index]
                const panelStyle = getComputedStyle(panel)
                const sidebar = panel.classList.contains('sidebar')
                // The panels start at the 36px band the workspace grid names, not at the title row plus the frame.
                // A framed card's box reaches up through its ring, so the card's hairline lies on the frameless seam.
                const ring = style.getPropertyValue('--card-ring').trim()
                const band = parseFloat(style.getPropertyValue('--titlebar-band'))
                const lift = parseFloat(ring) + band - parseFloat(style.getPropertyValue('--titlebar-height'))
                const card = framed && !sidebar
                fail(near(box.top, card ? band - lift : band), 'The top frame changed.')
                fail(near(box.bottom, innerHeight - frame), 'The bottom frame changed.')
                if (sidebar) {
                  fail(panelStyle.backgroundColor === (framed ? 'rgba(0, 0, 0, 0)' : sidebarPanel), `The sidebar has the wrong ground: ${panelStyle.backgroundColor}.`)
                  for (const edge of ['Top', 'Right', 'Bottom', 'Left']) fail(panelStyle[`border${edge}Width`] === '0px', 'The sidebar keeps a border.')
                  continue
                }
                fail(panelStyle.backgroundColor === surface, `${panel.className} lacks surface.`)
                for (const edge of ['Top', 'Right', 'Bottom', 'Left']) fail(panelStyle[`border${edge}Width`] === (card && edge === 'Top' ? `${lift}px` : '0px'), `${panel.className} has the wrong ${edge.toLowerCase()} border.`)
                // A framed card sits inside its box: a clip inset by the ring, and a hairline drawn as an outline inside the ring.
                const radius = style.getPropertyValue('--radius-panel').trim()
                fail(panelStyle.clipPath === (framed ? `inset(${ring} round ${radius})` : 'none'), `${panel.className} has the wrong clip: ${panelStyle.clipPath}.`)
                if (framed) fail(panelStyle.outlineColor === border && panelStyle.outlineWidth === '1px' && panelStyle.outlineOffset === `${-(parseFloat(ring) + 1)}px`, `${panel.className} lacks the card hairline.`)
                for (const corner of ['TopLeft', 'TopRight', 'BottomLeft', 'BottomRight']) {
                  fail(panelStyle[`border${corner}Radius`] === (framed ? `${parseFloat(radius) + parseFloat(ring)}px` : '0px'), `${panel.className} has the wrong radius.`)
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
                const tools = rect('.workspace-menu')
                fail(near(tools.right, innerWidth - 12), 'Workspace tools do not sit flush right in the title row.')
                fail(record.right <= tools.left, 'Record overlaps workspace tools.')
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
              // The title band is paper. The other edges and the gaps between panels take the frame color.
              fail(backgroundAt(innerWidth / 2, 1) === paper, 'The title band lacks paper.')
              // Hit testing takes whole pixels, and the outermost pixel row is frame in both styles.
              for (const [x, y] of [[0, innerHeight / 2], [innerWidth - 1, innerHeight / 2], [innerWidth / 2, innerHeight - 1]]) {
                fail(backgroundAt(x, y) === chrome, 'A window edge lacks its frame color.')
              }
              for (let index = sliding ? 2 : 1; index < boxes.length; index++) {
                for (let y = boxes[index].top + 1; y < boxes[index].bottom; y += 4) {
                  fail(backgroundAt(Math.floor(boxes[index - 1].right + frame / 2), y) === chrome, 'A panel gap lacks its frame color.')
                }
              }
            }
            const start = performance.now()
            do {
              await new Promise(requestAnimationFrame)
              check()
            } while (transition && performance.now() - start < 240)
            return [...new Set(failures)]
          }, [transition, frameName])
          assert.deepEqual(failures, [], `${frameName} ${platform} ${colorScheme} ${JSON.stringify(page.viewportSize())}`)
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
        console.log(`${frameName} checks passed for ${platform} in ${colorScheme}.`)
      } finally {
        await page.close()
      }
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
          // The actions hold voice, and a stop button during a turn. File attachment sits in the composer menu.
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

// Every agent avatar is a square box its drawing fills, centered in its slot: across a row's
// text, under a card's or header's center, and on its tile. Each surface shows one.
async function checkAgentAvatars(browser, baseUrl) {
  const surfaces = {
    '': ['.catalog-sidebar-row .side-action', '.agent-card .head-avatar', '.starter .thumb'],
    profile: ['.titlebar .agent-chat-title', '.profile .avatar', '.profile .check'],
    transcript: ['.agent-message header'],
    meeting: ['.agent-post header', '#agent-mentions [role="option"]'],
    approvals: ['.approval .who', '.running button'],
    import: ['.body .profile'],
    'group-create': ['.members .check'],
    move: ['.catalog-sidebar-row .side-action', '.profile .avatar'],
    'approval-inline': ['.approval .who'],
    preferences: ['.avatar-preview'],
  }
  for (const [colorScheme, avatars] of [['light', 'colorful'], ['dark', 'colorful'], ['light', 'muniment'], ['dark', 'muniment']]) {
    for (const [state, slots] of Object.entries(surfaces)) {
      const page = await browser.newPage({ viewport: { width: 1100, height: 720 }, colorScheme })
      try {
        await page.goto(`${baseUrl}/test/probe/agents.html?avatars=${avatars}&theme=${colorScheme}${state ? `&state=${state}` : ''}`)
        await page.waitForSelector('[data-probe-ready]')
        await page.evaluate(() => document.fonts.ready)
        if (state === 'meeting') {
          await page.locator('#composer-message').click()
          await page.keyboard.type('@')
          await page.waitForSelector('#agent-mentions [role="option"]')
        }
        const failures = await page.evaluate((slots) => {
          const failures = []
          const near = (a, b) => Math.abs(a - b) < 0.6
          const content = (element) => {
            const box = element.getBoundingClientRect()
            const style = getComputedStyle(element)
            const left = box.left + parseFloat(style.borderLeftWidth) + parseFloat(style.paddingLeft)
            const right = box.right - parseFloat(style.borderRightWidth) - parseFloat(style.paddingRight)
            const top = box.top + parseFloat(style.borderTopWidth) + parseFloat(style.paddingTop)
            const bottom = box.bottom - parseFloat(style.borderBottomWidth) - parseFloat(style.paddingBottom)
            return { x: (left + right) / 2, y: (top + bottom) / 2 }
          }
          for (const slot of slots) {
            const avatars = [...document.querySelectorAll(slot)].map((element) => element.querySelector(':scope > .agent-avatar')).filter(Boolean)
            if (!avatars.length) failures.push(`${slot} shows no avatar.`)
            for (const avatar of avatars) {
              const box = avatar.getBoundingClientRect()
              const drawing = avatar.querySelector('svg').getBoundingClientRect()
              const parent = avatar.parentElement
              const center = content(parent)
              const name = `${slot} ${box.width}px`
              if (box.width !== box.height || box.width < 16) failures.push(`${name} is not a square box.`)
              if (!['left', 'top', 'width', 'height'].every((side) => near(drawing[side], box[side]))) failures.push(`${name}: the drawing does not fill its box.`)
              const column = getComputedStyle(parent).flexDirection === 'column'
              // A row centers the avatar across its text; a card or header column centers it under the middle.
              if (column || slot === '.profile .avatar' || slot === '.starter .thumb') {
                if (!near(box.left + box.width / 2, center.x)) failures.push(`${name} is off center by ${(box.left + box.width / 2 - center.x).toFixed(2)}px across.`)
              }
              if (!column) {
                if (!near(box.top + box.height / 2, center.y)) failures.push(`${name} is off center by ${(box.top + box.height / 2 - center.y).toFixed(2)}px down.`)
                const text = avatar.nextElementSibling
                if (text && slot.includes('side-action')) {
                  const line = text.getBoundingClientRect()
                  if (!near(box.top + box.height / 2, line.top + line.height / 2)) failures.push(`${name} is off its row's text.`)
                }
              }
            }
          }
          return failures
        }, slots)
        assert.deepEqual(failures, [], `agents.html?state=${state} in ${colorScheme} with ${avatars} avatars`)
        // Each avatar draws the picked style in the theme's tone.
        const drawn = await page.evaluate(() => [...new Set([...document.querySelectorAll('.agent-avatar svg')].map((svg) => `${svg.dataset.style}:${svg.dataset.tone}`))])
        assert.deepEqual(drawn, [`${avatars}:${colorScheme}`], `agents.html?state=${state} draws ${drawn}`)
      } finally {
        await page.close()
      }
    }
  }
  const page = await browser.newPage({ viewport: { width: 1100, height: 720 } })
  try {
    await page.goto(`${baseUrl}/test/probe/avatars.html`)
    await page.waitForSelector('[data-probe-ready]')
    assert.equal(await page.getAttribute('body', 'data-seeds'), '48')
    // Each head's own bounds center on its 92px box, within a pixel and a quarter. The box a
    // browser measures leaves out strokes, and the generators center the painted head, so a
    // head whose top piece is a thick stroke measures up to half that stroke off.
    assert.ok(Number(await page.getAttribute('body', 'data-worst-offset')) < 1.25)
  } finally {
    await page.close()
  }
  console.log('Agent avatar checks passed.')
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
    await checkAgentAvatars(browser, baseUrl)
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
