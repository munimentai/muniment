import assert from 'node:assert/strict'

// No control moves during a session. Each check records the boxes of the
// composer band's controls and the sidebar head, walks the window through the
// states a session passes, and fails when a control leaves its box. The
// composer's action control comes and goes at the band's right end, and the
// band keeps its slot, so every other control stays put.
const TRACKED = {
  'composer box': '.composer',
  'composer plus': '.composer-meta > :first-child',
  'composer machine': '.composer-meta .machine-chip',
  'composer model': '.composer-meta [data-menu="model"]',
  'composer mode': '.composer-meta [data-menu="mode"]',
  'composer voice': '.composer-actions button[aria-label="Voice"]',
  'sidebar new': '.sidebar-head .new-thread',
  'sidebar search': '.sidebar-head .thread-search-field',
  'sidebar toggle': '.titlebar button[aria-label="Collapse sidebar"]',
  'sidebar settings': '.sidebar-actions button[aria-label="Settings"]',
}

const frames = (page) => page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))))

const boxes = (page) => page.evaluate((tracked) => Object.fromEntries(Object.entries(tracked).map(([name, selector]) => {
  const element = document.querySelector(selector)
  if (!element) return [name, null]
  const { x, y, width, height } = element.getBoundingClientRect()
  return [name, { x, y, width, height }]
})), TRACKED)

function compare(base, next, state, fixture) {
  for (const [name, box] of Object.entries(base)) {
    if (!box || !next[name]) continue
    const moved = ['x', 'y', 'width', 'height'].filter((side) => Math.abs(box[side] - next[name][side]) > 0.5)
    assert.deepEqual(moved, [], `${fixture}: ${name} moved after ${state}: ${JSON.stringify(box)} became ${JSON.stringify(next[name])}`)
  }
}

async function walk(browser, baseUrl, fixture, steps) {
  const context = await browser.newContext({ viewport: { width: 1100, height: 720 }, reducedMotion: 'reduce' })
  const page = await context.newPage()
  try {
    await page.goto(`${baseUrl}/test/probe/${fixture}`)
    await page.waitForSelector('[data-probe-ready]')
    await page.evaluate(() => document.fonts.ready)
    await frames(page)
    const base = await boxes(page)
    for (const name of ['composer box', 'composer voice', 'sidebar new', 'sidebar search']) assert.ok(base[name], `${fixture}: ${name} is missing`)
    for (const [state, step] of steps) {
      await step(page)
      await frames(page)
      compare(base, await boxes(page), state, fixture)
    }
  } finally {
    await context.close()
  }
}

const emit = (page, payload) => page.evaluate((payload) => window.__PROBE__.emit('chat-event', payload), payload)

export async function checkLayoutStability(browser, baseUrl) {
  await walk(browser, baseUrl, 'history.html', [
    ['hovering a thread row', (page) => page.locator('#sidebar .thread-row-title').first().hover()],
    ['focusing the composer', (page) => page.locator('#composer-message').focus()],
    ['typing a draft', (page) => page.locator('#composer-message').pressSequentially('Draft a reminder')],
    ['clearing the draft', (page) => page.locator('#composer-message').fill('')],
    ['hovering a composer menu', (page) => page.locator('.composer-meta button').first().hover()],
    ['opening and closing Settings', async (page) => {
      await page.getByRole('button', { name: 'Settings', exact: true }).click()
      await page.getByRole('button', { name: 'Back to threads' }).click()
      await page.locator('.sidebar-head .new-thread').waitFor()
    }],
  ])
  await walk(browser, baseUrl, 'in-flight.html', [
    ['streaming a chunk', (page) => emit(page, { runId: 'probe-in-flight', threadId: 'probe-thread', type: 'text-delta', text: ' and Halden renews monthly.' })],
    ['streaming more chunks', async (page) => { for (let index = 0; index < 10; index += 1) await emit(page, { runId: 'probe-in-flight', threadId: 'probe-thread', type: 'text-delta', text: ` clause ${index}.` }) }],
    ['settling the reply', (page) => emit(page, { runId: 'probe-in-flight', threadId: 'probe-thread', type: 'completed', receipt: { time: '4.1s' } })],
    ['typing the next draft', async (page) => {
      await page.waitForFunction(() => !document.querySelector('.composer-action.stop'))
      await page.locator('#composer-message').pressSequentially('Next')
    }],
  ])
  console.log('Layout stability checks passed.')
}
