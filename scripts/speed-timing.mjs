#!/usr/bin/env node
// Times the desktop on one machine and compares the numbers to the speed
// budgets in scripts/speed-budgets.json. Wall time is noisy on shared runners,
// so this runs nightly on one dedicated Mac or by hand, never as a pull request
// gate. docs/speed-budgets.md explains each number and how to run it.
//
//   node scripts/speed-timing.mjs --app <binary>   launch the e2e WebDriver build and time it
//   node scripts/speed-timing.mjs --webdriver <url> time a running e2e build
//   node scripts/speed-timing.mjs --probe           time the probe pages in Chromium
//
// Options: --out <file> (speed-report.json), --idle <seconds> (60), --prompt <text>,
// --home <dir>, --port <n> (4445), --baseline <report>, --strict.
import { spawn, execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
export const BUDGETS = JSON.parse(fs.readFileSync(path.join(ROOT, 'scripts/speed-budgets.json'), 'utf8'))
// A nightly number this far over the last report's fails, as a regression, under --strict.
export const REGRESSION = 0.1

export function parseArgs(argv) {
  const options = { out: 'speed-report.json', idle: 60, port: 4445, strict: false }
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index]
    const value = () => argv[++index]
    if (flag === '--app') options.app = value()
    else if (flag === '--webdriver') options.webdriver = value()
    else if (flag === '--probe') options.probe = true
    else if (flag === '--out') options.out = value()
    else if (flag === '--idle') options.idle = Number(value())
    else if (flag === '--prompt') options.prompt = value()
    else if (flag === '--home') options.home = value()
    else if (flag === '--port') options.port = Number(value())
    else if (flag === '--baseline') options.baseline = value()
    else if (flag === '--strict') options.strict = true
    else throw new Error(`Unknown option: ${flag}`)
  }
  if ([options.app, options.webdriver, options.probe].filter(Boolean).length !== 1) throw new Error('Pass one of --app, --webdriver or --probe.')
  if (!Number.isFinite(options.idle) || options.idle < 0) throw new Error('--idle takes a number of seconds.')
  return options
}

export function percentile(values, share) {
  if (!values.length) return null
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.ceil(share * sorted.length) - 1)]
}

// `ps -o pid=,time=,rss=` lines: CPU time as [[dd-]hh:]mm:ss[.cc] and RSS in KiB.
export function parsePs(text) {
  const seconds = (time) => {
    const [days, clock] = time.includes('-') ? time.split('-') : ['0', time]
    return Number(days) * 86_400 + clock.split(':').reduce((total, part) => total * 60 + Number(part), 0)
  }
  return text.trim().split('\n').filter(Boolean).map((line) => {
    const [pid, time, rss] = line.trim().split(/\s+/)
    return { pid: Number(pid), cpuSeconds: seconds(time), rssKiB: Number(rss) }
  })
}

// macOS `footprint` prints a summary line such as `Footprint: 182 MB`.
export function parseFootprint(text) {
  const match = text.match(/footprint:\s*([\d.]+)\s*(KB|MB|GB)/i)
  if (!match) return null
  return Number(match[1]) * { KB: 1 / 1024, MB: 1, GB: 1024 }[match[2].toUpperCase()]
}

// Each measured metric beside its budget, and beside the baseline when one is given.
export function compare(metrics, budgets = BUDGETS, baseline = null) {
  return Object.entries(budgets).map(([metric, budget]) => {
    const value = metrics[metric] ?? null
    const before = baseline?.metrics?.[metric] ?? null
    const overBudget = value !== null && value > budget
    const regressed = value !== null && before !== null && before > 0 && value > before * (1 + REGRESSION)
    return { metric, value, budget, baseline: before, pass: value === null ? null : !overBudget && !regressed }
  })
}

// In-page measurements. Each is an async function of the page and runs the
// same way through WebDriver's async execute and through Playwright.
const PAGE = {
  // The composer mark, from the page's own time origin.
  composer: async () => {
    const started = performance.now()
    while (!performance.getEntriesByName('muniment:composer-ready').length) {
      if (performance.now() - started > 60_000) return null
      await new Promise((resolve) => setTimeout(resolve, 20))
    }
    const mark = performance.getEntriesByName('muniment:composer-ready')[0]
    return { origin: performance.timeOrigin, sinceOrigin: mark.startTime }
  },
  // Each key: the value grows by one character and the input event runs. The
  // work ends once the app's effects settle and the page lays out again, which
  // is the part of keystroke to paint the app owns. The frame time adds the wait
  // for the next frame. The draft returns to what it held.
  keystrokes: async (count) => {
    const field = document.querySelector('#composer-message')
    if (!field) return null
    const before = field.value
    field.focus()
    const frame = () => new Promise((resolve) => requestAnimationFrame(() => setTimeout(resolve, 0)))
    await frame()
    const work = []
    const frames = []
    for (let index = 0; index < count; index += 1) {
      const started = performance.now()
      field.value += 'abcdefghij'[index % 10]
      field.dispatchEvent(new InputEvent('input', { bubbles: true, data: 'x', inputType: 'insertText' }))
      await Promise.resolve()
      await Promise.resolve()
      void document.body.offsetHeight
      work.push(performance.now() - started)
      await frame()
      frames.push(performance.now() - started)
    }
    field.value = before
    field.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }))
    return { work, frames }
  },
  // Sends the prompt and times the send, the first text event and the first painted token.
  firstToken: async (prompt) => {
    const field = document.querySelector('#composer-message')
    if (!field) return null
    let firstEvent = null
    const stop = await window.__TAURI__?.event?.listen?.('chat-event', ({ payload }) => { if (firstEvent === null && payload?.type === 'text-delta') firstEvent = performance.now() })
    field.focus()
    field.value = prompt
    field.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText' }))
    await new Promise((resolve) => requestAnimationFrame(resolve))
    const shown = () => [...document.querySelectorAll('.response .streaming .assistant-markdown, .response .assistant-markdown')].at(-1)?.textContent?.trim()
    const before = shown()
    const sent = performance.now()
    document.querySelector('.composer-action[aria-label="Send"]')?.click()
    const painted = await new Promise((resolve) => {
      const check = () => {
        const text = shown()
        if (text && text !== before) requestAnimationFrame(() => resolve(performance.now()))
        else if (performance.now() - sent > 120_000) resolve(null)
        else requestAnimationFrame(check)
      }
      check()
    })
    stop?.()
    return { sendToPaint: painted === null ? null : painted - sent, eventToPaint: painted === null || firstEvent === null ? null : painted - firstEvent }
  },
}

// A minimal WebDriver client: the e2e build serves the protocol, and on macOS
// only the async execute answers, so every script goes through it.
function webdriver(base) {
  const call = async (method, route, body) => {
    const response = await fetch(`${base}${route}`, { method, headers: { 'Content-Type': 'application/json' }, body: body ? JSON.stringify(body) : undefined })
    const answer = await response.json()
    if (!response.ok || answer.value?.error) throw new Error(`WebDriver ${route}: ${answer.value?.message ?? response.status}`)
    return answer.value
  }
  let session
  return {
    async ready(timeout = 60_000) {
      const started = Date.now()
      for (;;) {
        try { if ((await call('GET', '/status')).ready !== false) return } catch (_) {}
        if (Date.now() - started > timeout) throw new Error('The WebDriver server did not answer.')
        await new Promise((resolve) => setTimeout(resolve, 50))
      }
    },
    async open() {
      session = (await call('POST', '/session', { capabilities: { alwaysMatch: {} } })).sessionId
      const handles = await call('GET', `/session/${session}/window/handles`)
      if (handles.includes('main')) await call('POST', `/session/${session}/window`, { handle: 'main' })
    },
    run(fn, ...args) {
      const script = `const done = arguments[arguments.length - 1]; (${fn})(...Array.from(arguments).slice(0, -1)).then(done, (error) => done({ error: String(error) }))`
      return call('POST', `/session/${session}/execute/async`, { script, args })
    },
    close: () => session && call('DELETE', `/session/${session}`).catch(() => {}),
  }
}

// The processes the desktop runs: the app's own tree, and the runtime, which
// may run as a background service outside that tree.
function processes(appPid) {
  const pids = new Set(appPid ? [appPid] : [])
  const children = (pid) => { try { return execFileSync('pgrep', ['-P', String(pid)], { encoding: 'utf8' }).split('\n').filter(Boolean).map(Number) } catch (_) { return [] } }
  for (const pid of [...pids]) for (const child of children(pid)) { pids.add(child); for (const grandchild of children(child)) pids.add(grandchild) }
  try { for (const pid of execFileSync('pgrep', ['-f', 'muniment-runtime'], { encoding: 'utf8' }).split('\n').filter(Boolean)) pids.add(Number(pid)) } catch (_) {}
  if (!appPid) try { for (const pid of execFileSync('pgrep', ['-x', 'muniment'], { encoding: 'utf8' }).split('\n').filter(Boolean)) pids.add(Number(pid)) } catch (_) {}
  return [...pids].filter((pid) => pid > 0 && pid !== process.pid)
}

function sample(pids) {
  if (!pids.length) return []
  try { return parsePs(execFileSync('ps', ['-o', 'pid=,time=,rss=', '-p', pids.join(',')], { encoding: 'utf8' })) } catch (_) { return [] }
}

function footprint(pids) {
  if (process.platform !== 'darwin') return null
  let total = 0
  for (const pid of pids) {
    try { total += parseFootprint(execFileSync('footprint', [String(pid)], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })) ?? 0 } catch (_) { return null }
  }
  return total
}

// CPU over the idle window, as a percent of one core, and memory at its end.
async function idle(seconds, appPid) {
  const pids = processes(appPid)
  const first = sample(pids)
  await new Promise((resolve) => setTimeout(resolve, seconds * 1000))
  const last = sample(pids)
  const cpu = last.reduce((total, entry) => total + entry.cpuSeconds - (first.find(({ pid }) => pid === entry.pid)?.cpuSeconds ?? entry.cpuSeconds), 0)
  return {
    pids: last.map(({ pid }) => pid),
    idleCpuPercent: seconds ? Math.round(cpu / seconds * 100 * 100) / 100 : null,
    idleRssMb: last.length ? Math.round(last.reduce((total, entry) => total + entry.rssKiB, 0) / 1024) : null,
    idleFootprintMb: footprint(last.map(({ pid }) => pid)),
  }
}

async function measure(run, options, launched = null) {
  const metrics = {}
  const composer = await run(PAGE.composer)
  if (composer?.sinceOrigin != null) {
    metrics.pageToComposerMs = Math.round(composer.sinceOrigin)
    if (launched) metrics.launchToComposerMs = Math.round(composer.origin + composer.sinceOrigin - launched)
  }
  const keys = await run(PAGE.keystrokes, 60)
  if (Array.isArray(keys?.work)) {
    const tenth = (value) => Math.round(value * 10) / 10
    metrics.keystrokeP50Ms = tenth(percentile(keys.work, 0.5))
    metrics.keystrokeP95Ms = tenth(percentile(keys.work, 0.95))
    metrics.keystrokeFrameP95Ms = tenth(percentile(keys.frames, 0.95))
  }
  if (options.prompt) {
    const token = await run(PAGE.firstToken, options.prompt)
    if (token?.sendToPaint != null) metrics.sendToFirstTokenMs = Math.round(token.sendToPaint)
    if (token?.eventToPaint != null) metrics.firstTokenEventToPaintMs = Math.round(token.eventToPaint)
  }
  return metrics
}

async function probe(options) {
  const { chromium } = await import('playwright')
  const { createServer } = await import('node:http')
  const types = { '.css': 'text/css', '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json', '.svg': 'image/svg+xml', '.woff2': 'font/woff2' }
  const server = createServer((request, response) => {
    const pathname = decodeURIComponent(new URL(request.url, 'http://127.0.0.1').pathname)
    const file = path.resolve(ROOT, pathname.startsWith('/assets/') ? `dist${pathname}` : pathname.slice(1))
    if (!file.startsWith(ROOT) || !fs.existsSync(file) || !fs.statSync(file).isFile()) return response.writeHead(404).end()
    response.writeHead(200, { 'Content-Type': types[path.extname(file)] ?? 'application/octet-stream' }).end(fs.readFileSync(file))
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const browser = await chromium.launch()
  try {
    const page = await browser.newPage({ viewport: { width: 1100, height: 720 } })
    const launched = Date.now()
    await page.goto(`http://127.0.0.1:${server.address().port}/test/probe/history.html`)
    const metrics = await measure((fn, ...args) => page.evaluate(([source, args]) => (0, eval)(`(${source})`)(...args), [fn.toString(), args]), options, launched)
    // The probe has no desktop processes, so idle CPU and memory stay unmeasured.
    return metrics
  } finally {
    await browser.close()
    server.close()
  }
}

async function desktop(options) {
  let child = null
  let launched = null
  const base = options.webdriver ?? `http://127.0.0.1:${options.port}`
  if (options.app) {
    const env = { ...process.env, TAURI_WEBDRIVER_PORT: String(options.port) }
    // An isolated home keeps the run off the owner's profile and logs.
    if (options.home) Object.assign(env, { HOME: options.home, MUNIMENT_LOG_DIR: path.join(options.home, 'logs') })
    launched = Date.now()
    child = spawn(options.app, [], { env, stdio: 'ignore' })
  }
  const driver = webdriver(base)
  try {
    await driver.ready()
    await driver.open()
    const metrics = await measure(driver.run, options, launched)
    Object.assign(metrics, await idle(options.idle, child?.pid))
    return metrics
  } finally {
    await driver.close()
    child?.kill()
  }
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseArgs(argv)
  const metrics = options.probe ? await probe(options) : await desktop(options)
  const baseline = options.baseline ? JSON.parse(fs.readFileSync(options.baseline, 'utf8')) : null
  const results = compare(metrics, BUDGETS, baseline)
  const report = {
    at: new Date().toISOString(),
    mode: options.probe ? 'probe' : options.app ? 'launch' : 'attach',
    platform: `${os.platform()} ${os.arch()} ${os.release()}`,
    cpus: os.cpus()[0]?.model ?? null,
    memoryGb: Math.round(os.totalmem() / 2 ** 30),
    metrics,
    results,
  }
  fs.writeFileSync(options.out, `${JSON.stringify(report, null, 2)}\n`)
  for (const { metric, value, budget, baseline: before, pass } of results) {
    console.log(`${pass === null ? 'skip' : pass ? 'pass' : 'FAIL'}  ${metric.padEnd(26)} ${value ?? '-'} (budget ${budget}${before !== null ? `, last ${before}` : ''})`)
  }
  console.log(`Report: ${options.out}`)
  if (options.strict && results.some(({ pass }) => pass === false)) process.exitCode = 1
  return report
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => { console.error(error.message); process.exitCode = 1 })
}
