import assert from 'node:assert/strict'

// Speed budgets that runner noise cannot move: requests, DOM writes and
// rendered nodes, counted in headless Chromium against the probe fixtures.
// Wall time stays out of the pull request gate. scripts/speed-timing.mjs
// times the installed app on one Mac each night. docs/speed-budgets.md lists
// every budget and how each number is read.
export const SPEED_BUDGETS = {
  // Requests the window sends from its first frame until it settles, per fixture.
  start: {
    'index.html': { total: 48, sidebar_arrange: 2 },
    'history.html': { total: 55, sidebar_arrange: 5, chat_thread_open: 1 },
  },
  // DOM records per streamed text chunk outside the reply that streams, and inside it.
  chunk: { outside: 0, inside: 4, requests: 0, sidebar_arrange: 0 },
  // Requests core's `sidebar::arrange` answers per settled reply.
  settle: { sidebar_arrange: 1 },
  // DOM records per keystroke outside the composer, and inside it.
  keystroke: { outside: 0, inside: 3, requests: 0 },
  // A 5,000-turn thread opens with the work of a 500-turn one.
  longThread: { turns: 5_000, items: 120, elements: 2_000, markdown: 60 },
}

const frames = (page, count = 2) => page.evaluate((count) => new Promise((resolve) => {
  let left = count
  const next = () => (--left <= 0 ? resolve() : requestAnimationFrame(next))
  requestAnimationFrame(next)
}), count)

async function open(browser, baseUrl, fixture) {
  // Reduced motion stills the run mark, so its animation frames never count as writes.
  const context = await browser.newContext({ viewport: { width: 1100, height: 720 }, reducedMotion: 'reduce' })
  const page = await context.newPage()
  await page.goto(`${baseUrl}/test/probe/${fixture}`)
  await page.waitForSelector('[data-probe-ready]', { timeout: 30_000 })
  await frames(page, 3)
  return { page, close: () => context.close() }
}

const commands = (page, from = 0) => page.evaluate((from) => window.__PROBE__.invokedCommands.slice(from).map(({ command }) => command), from)
const commandCount = (page) => page.evaluate(() => window.__PROBE__.invokedCommands.length)
const tally = (list) => list.reduce((counts, command) => ({ ...counts, [command]: (counts[command] ?? 0) + 1 }), {})
const actionRequests = (list) => list.filter((command) => !POLLS.has(command))

// Polls and clocks run on their own time, not on the action being counted:
// a run's elapsed time ticks each second.
const POLLS = new Set(['runtime_state'])
const CLOCKS = '.receipt-time'

// Records every DOM write under the app until `stop`, and sorts each by
// whether it landed inside `inside`, a selector, or in a node it removed.
async function watchWrites(page, inside) {
  await page.evaluate(([inside, clocks]) => {
    const state = { inside: 0, outside: 0, notes: [] }
    const observer = new MutationObserver((records) => {
      for (const record of records) {
        const target = record.target.nodeType === Node.ELEMENT_NODE ? record.target : record.target.parentElement
        if (target?.closest(clocks)) continue
        const within = !target?.isConnected || !!target.closest(inside)
        if (within) state.inside += 1
        else {
          state.outside += 1
          if (state.notes.length < 8) state.notes.push(`${record.type}:${record.attributeName ?? ''} on ${target?.tagName}.${target?.className?.baseVal ?? target?.className}`)
        }
      }
    })
    observer.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true })
    window.__speedWrites = { state, observer }
  }, [inside, CLOCKS])
  return () => page.evaluate(() => {
    const { state, observer } = window.__speedWrites
    for (const record of observer.takeRecords()) void record
    observer.disconnect()
    return state
  })
}

async function checkStart(browser, baseUrl, report) {
  for (const [fixture, budget] of Object.entries(SPEED_BUDGETS.start)) {
    const { page, close } = await open(browser, baseUrl, fixture)
    try {
      // The window settles: every request its first frames started has answered.
      await page.waitForTimeout(1_000)
      const sent = actionRequests(await commands(page))
      const counts = tally(sent)
      report[`start ${fixture}`] = { total: sent.length, ...counts }
      assert.ok(sent.length <= budget.total, `${fixture} sent ${sent.length} requests at start, budget ${budget.total}: ${JSON.stringify(counts)}`)
      for (const [command, cap] of Object.entries(budget)) {
        if (command === 'total') continue
        assert.ok((counts[command] ?? 0) <= cap, `${fixture} sent ${command} ${counts[command]} times at start, budget ${cap}`)
      }
      // An identical arrangement is never asked twice in a row.
      const inputs = await page.evaluate(() => window.__PROBE__.invokedCommands.filter(({ command }) => command === 'sidebar_arrange').map(({ payload }) => JSON.stringify(payload.input)))
      assert.ok(inputs.every((input, index) => index === 0 || input !== inputs[index - 1]), `${fixture} asked sidebar_arrange twice with the same input`)
    } finally {
      await close()
    }
  }
}

async function checkStreaming(browser, baseUrl, report) {
  const { page, close } = await open(browser, baseUrl, 'in-flight.html')
  try {
    const chunks = 30
    const from = await commandCount(page)
    const stop = await watchWrites(page, '.response:has(.streaming)')
    for (let index = 0; index < chunks; index += 1) {
      await page.evaluate((index) => window.__PROBE__.emit('chat-event', { runId: 'probe-in-flight', threadId: 'probe-thread', type: 'text-delta', text: ` clause ${index}` }), index)
      await frames(page)
    }
    const writes = await stop()
    const sent = actionRequests(await commands(page, from))
    report.chunk = { outside: writes.outside / chunks, inside: writes.inside / chunks, requests: sent.length / chunks, sidebar_arrange: sent.filter((command) => command === 'sidebar_arrange').length / chunks }
    assert.ok(writes.outside <= SPEED_BUDGETS.chunk.outside * chunks, `streamed chunks wrote ${writes.outside} times outside the reply: ${writes.notes.join(', ')}`)
    assert.ok(writes.inside <= SPEED_BUDGETS.chunk.inside * chunks, `streamed chunks wrote ${writes.inside} times inside the reply, budget ${SPEED_BUDGETS.chunk.inside} per chunk`)
    assert.ok(sent.length <= SPEED_BUDGETS.chunk.requests * chunks, `streamed chunks sent ${JSON.stringify(tally(sent))}`)
    assert.ok(await page.locator('.response .streaming').textContent().then((text) => text.includes(`clause ${chunks - 1}`)))

    // The settled reply asks core for one arrangement at most.
    const settledFrom = await commandCount(page)
    await page.evaluate(() => window.__PROBE__.emit('chat-event', { runId: 'probe-in-flight', threadId: 'probe-thread', type: 'completed', receipt: { time: '4.1s' } }))
    await page.waitForTimeout(300)
    const settled = actionRequests(await commands(page, settledFrom))
    report.settle = { sidebar_arrange: settled.filter((command) => command === 'sidebar_arrange').length, requests: tally(settled) }
    assert.ok(report.settle.sidebar_arrange <= SPEED_BUDGETS.settle.sidebar_arrange, `a settled reply asked sidebar_arrange ${report.settle.sidebar_arrange} times`)
  } finally {
    await close()
  }
}

async function checkKeystrokes(browser, baseUrl, report) {
  const { page, close } = await open(browser, baseUrl, 'history.html')
  try {
    const composer = page.locator('#composer-message')
    await composer.focus()
    // The first key shows the send control, a write the budget allows once.
    await composer.press('W')
    await frames(page)
    const from = await commandCount(page)
    const stop = await watchWrites(page, '.composer')
    const keys = 'rite the renewal'
    for (const key of keys) {
      await composer.press(key === ' ' ? 'Space' : key)
      await frames(page)
    }
    const writes = await stop()
    const sent = actionRequests(await commands(page, from))
    report.keystroke = { outside: writes.outside / keys.length, inside: writes.inside / keys.length, requests: sent.length / keys.length }
    assert.equal(await composer.inputValue(), 'Write the renewal')
    assert.ok(writes.outside <= SPEED_BUDGETS.keystroke.outside * keys.length, `keystrokes wrote ${writes.outside} times outside the composer: ${writes.notes.join(', ')}`)
    assert.ok(writes.inside <= SPEED_BUDGETS.keystroke.inside * keys.length, `keystrokes wrote ${writes.inside} times inside the composer`)
    assert.ok(sent.length <= SPEED_BUDGETS.keystroke.requests * keys.length, `keystrokes sent ${JSON.stringify(tally(sent))}`)
  } finally {
    await close()
  }
}

const transcriptShape = (page) => page.evaluate(() => {
  const thread = document.querySelector('.thread')
  return {
    items: thread.querySelectorAll(':scope > .user-turn, :scope > .response, :scope > .agent-turn, :scope > .silent-note').length,
    elements: thread.querySelectorAll('*').length,
    markdown: thread.querySelectorAll('.assistant-markdown').length,
  }
})

async function checkLongThread(browser, baseUrl, report) {
  const budget = SPEED_BUDGETS.longThread
  const shapes = {}
  for (const turns of [budget.turns / 10, budget.turns]) {
    const { page, close } = await open(browser, baseUrl, `long-thread.html?turns=${turns}`)
    try {
      const pages = (await commands(page)).filter((command) => command === 'chat_thread_open').length
      shapes[turns] = await transcriptShape(page)
      report[`open ${turns} turns`] = { ...shapes[turns], historyPages: pages }
      assert.equal(pages, Math.ceil(turns / 100), `${turns} turns read ${pages} history pages`)
      assert.ok(shapes[turns].items <= budget.items && shapes[turns].elements <= budget.elements && shapes[turns].markdown <= budget.markdown, `opening ${turns} turns rendered ${JSON.stringify(shapes[turns])}`)
      if (turns !== budget.turns) continue
      // Opening the long thread renders what the short one renders.
      assert.deepEqual(shapes[turns], shapes[budget.turns / 10])
      // Scrolling to the top again and again keeps the window bounded.
      let largest = shapes[turns]
      for (let step = 0; step < 24; step += 1) {
        await page.evaluate(() => { document.querySelector('.thread').scrollTop = 0 })
        await frames(page, 3)
        const shape = await transcriptShape(page)
        if (shape.elements > largest.elements) largest = shape
        assert.ok(shape.items <= budget.items && shape.elements <= budget.elements, `scrolling up rendered ${JSON.stringify(shape)}`)
      }
      report[`scrolled ${turns} turns`] = largest
      // The window moved up: the newest reply left the DOM, an older one arrived.
      const text = await page.locator('.thread').textContent()
      assert.ok(!text.includes(`Reply ${turns} is done.`), 'the newest reply stayed rendered after scrolling up')
      const oldest = Number(text.match(/Question (\d+):/)?.[1])
      assert.ok(oldest <= turns - 200, `scrolling up reached question ${oldest} only`)
      report[`scrolled ${turns} turns`].oldest = oldest
      await page.getByRole('button', { name: 'Jump to latest', exact: true }).click()
      await page.waitForFunction((turns) => document.querySelector('.thread').textContent.includes(`Reply ${turns} is done.`), turns)
      const back = await transcriptShape(page)
      assert.ok(back.items <= budget.items, `jumping to the latest rendered ${JSON.stringify(back)}`)
    } finally {
      await close()
    }
  }
}

export async function checkSpeedBudgets(browser, baseUrl) {
  const report = {}
  await checkStart(browser, baseUrl, report)
  await checkStreaming(browser, baseUrl, report)
  await checkKeystrokes(browser, baseUrl, report)
  await checkLongThread(browser, baseUrl, report)
  console.log(`Speed budget checks passed: ${JSON.stringify(report)}`)
  return report
}
