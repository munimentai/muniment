import { afterEach, expect, it } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { createRawSnippet, flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import appSource from '../App.svelte?raw'
import ChatComposer from './ChatComposer.svelte'
import ComposerMenuButton from './ComposerMenuButton.svelte'
import ContextMeter from '../lib/ContextMeter.svelte'
import { COLLAPSE_ORDER, MIN_MODEL_NAME, fitComposerMeta } from './composer-priority.js'
import { compile } from 'svelte/compiler'

let components = []
afterEach(() => {
  for (const component of components) unmount(component)
  components = []
  document.body.replaceChildren()
})

// The composer band's own rules, as the build compiles App.svelte, applied to
// the same class names outside the component's scope. The compiler drops a
// rule no markup in App.svelte can match, so a dropped rule fails here too.
const appCss = compile(appSource, { filename: 'src/App.svelte', css: 'external', cssHash: () => 'app-scope' }).css.code
const bandRules = [...appCss.replace(/\/\*[\s\S]*?\*\//g, '').replaceAll(/:where\(\.app-scope\)|\.app-scope/g, '').matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .filter(([, selector]) => /^\s*\.composer-(row|meta|actions|icon|action)\b/.test(selector) && !selector.includes('@'))
  .map(([, selector, body]) => `${selector.trim()} {${body}}`)
  .join('\n')

const glyph = (name) => createRawSnippet(() => ({ render: () => `<svg class="lucide" data-icon="${name}" width="14" height="14"></svg>` }))
const longModel = 'GPT-5.6 Terra (unavailable)'

// The composer in a thread column `column` pixels wide: Extensions, the
// machine, model, thinking and mode chips, then Voice and the Send slot.
function composer(column, { model = longModel, context = false } = {}) {
  const style = document.createElement('style')
  style.textContent = bandRules
  document.head.append(style)
  const thread = document.createElement('div')
  thread.style.cssText = `display: grid; width: ${column}px;`
  document.body.append(thread)
  const shell = mount(ChatComposer, { target: thread, props: { children: createRawSnippet(() => ({ render: () => '<div class="composer-row"><div class="composer-meta"><button type="button" aria-label="Extensions" style="flex: none; width: 24px; height: 24px"></button></div><div class="composer-actions"><button type="button" class="quiet composer-icon" aria-label="Voice"><svg width="16" height="16"></svg></button><span class="composer-action-slot"></span></div></div>' })) } })
  components.push(shell)
  flushSync()
  const meta = thread.querySelector('.composer-meta')
  components.push(mount(ComposerMenuButton, { target: meta, props: { menu: 'machine', chevrons: true, label: 'Machine: This computer. Choose a machine', text: 'This computer', icon: glyph('monitor') } }))
  components.push(mount(ComposerMenuButton, { target: meta, props: { menu: 'model', chevrons: true, haspopup: 'dialog', label: `Model: ${model}`, text: model, icon: glyph('openai') } }))
  components.push(mount(ComposerMenuButton, { target: meta, props: { menu: 'thinking', label: 'Thinking level: Medium', text: 'Medium', icon: glyph('brain') } }))
  components.push(mount(ComposerMenuButton, { target: meta, props: { menu: 'mode', chevrons: true, label: 'Permission mode: Ask before changes', text: 'Ask before changes', icon: glyph('hand') } }))
  if (context) components.push(mount(ContextMeter, { target: meta, props: { usage: { windowKnown: true, tokens: 70000, contextWindow: 200000, percent: 35 } } }))
  flushSync()
  fitComposerMeta(meta)
  return thread
}

// The default 1280px window with a 195px sidebar leaves the thread 1085px,
// an open agent profile at its 320px default leaves it 765px, one widened to
// 445px leaves it 640px, where the permission label gives way first, and one
// widened to 525px leaves it 560px, where the model name must give way.
for (const [state, column, clips] of [['alone', 1085, false], ['beside the agent profile', 765, false], ['beside a wider agent profile', 640, false], ['beside the widest agent profile', 560, false]]) {
  it(`keeps Voice on the chips' line with a long model name, ${state} at the default window width`, async () => {
    await page.viewport(1280, 800)
    const thread = composer(column)
    await document.fonts.ready
    const row = thread.querySelector('.composer-row')
    const chip = thread.querySelector('[data-menu="model"]')
    const voice = thread.querySelector('[aria-label="Voice"]')
    const middle = (element) => { const box = element.getBoundingClientRect(); return box.top + box.height / 2 }
    // One line: Voice sits level with the model chip, and the row is one control tall.
    expect(Math.abs(middle(voice) - middle(chip))).toBeLessThan(1)
    const tallest = Math.max(...[...row.querySelectorAll('button, .composer-action-slot')].map((element) => element.getBoundingClientRect().height))
    expect(row.getBoundingClientRect().height).toBeLessThanOrEqual(tallest + 0.5)
    expect(voice.getBoundingClientRect().right).toBeLessThanOrEqual(row.getBoundingClientRect().right + 0.5)
    // No chip runs under the actions.
    expect(thread.querySelector('.composer-meta').scrollWidth).toBeLessThanOrEqual(thread.querySelector('.composer-meta').clientWidth)
    // The model chip keeps its cap, and a name it clips ends in an ellipsis.
    expect(chip.getBoundingClientRect().width).toBeLessThanOrEqual(200)
    const label = chip.querySelector('.label')
    expect(label.scrollWidth > label.clientWidth).toBe(clips)
    if (!clips) return
    expect(getComputedStyle(label).textOverflow).toBe('ellipsis')
    // Hover shows the whole name in the app's tooltip.
    expect(getComputedStyle(chip, '::after').display).toBe('none')
    await userEvent.hover(chip)
    await expect.poll(() => getComputedStyle(chip, '::after').display).toBe('block')
    expect(getComputedStyle(chip, '::after').content).toBe(`"Model: ${longModel}"`)
    await expect.poll(() => getComputedStyle(chip, '::after').opacity).toBe('1')
  })
}

it('shows no hover label on a model name that fits', async () => {
  await page.viewport(1280, 800)
  const thread = composer(1085)
  await document.fonts.ready
  const chip = thread.querySelector('[data-menu="thinking"]')
  await userEvent.hover(chip)
  expect(chip.dataset.clipped).toBeUndefined()
  expect(getComputedStyle(chip, '::after').display).toBe('none')
})

// With the context meter showing beside an open agent profile, the percent
// and then the permission label give way before a short model name, which
// read "GPT-6 …" in a 720px thread before.
for (const [column, collapse] of [[1085, undefined], [720, '1'], [660, '2']]) {
  it(`keeps a short model name readable beside the context meter in a ${column}px thread`, async () => {
    await page.viewport(1280, 800)
    const thread = composer(column, { model: 'GPT-6 Astra', context: true })
    await document.fonts.ready
    const meta = thread.querySelector('.composer-meta')
    fitComposerMeta(meta)
    expect(meta.dataset.collapse).toBe(collapse)
    const label = thread.querySelector('[data-menu="model"] .label')
    expect(label.scrollWidth).toBeLessThanOrEqual(label.clientWidth)
    // No control runs past the meta side into the actions.
    for (const control of meta.children) expect(control.getBoundingClientRect().right).toBeLessThanOrEqual(meta.getBoundingClientRect().right + 0.5)
    const percent = thread.querySelector('.context-trigger > span')
    const mode = thread.querySelector('[data-menu="mode"]')
    expect(percent.getClientRects().length > 0).toBe(!collapse)
    expect(mode.querySelector('.label').getClientRects().length > 0).toBe(collapse !== '2')
    // A collapsed control keeps its name and its hover label.
    expect(mode).toHaveAttribute('aria-label', 'Permission mode: Ask before changes')
    if (collapse === '2') {
      await userEvent.hover(mode)
      await expect.poll(() => getComputedStyle(mode, '::after').display).toBe('block')
    }
    const voice = thread.querySelector('[aria-label="Voice"]')
    const middle = (element) => { const box = element.getBoundingClientRect(); return box.top + box.height / 2 }
    expect(Math.abs(middle(voice) - middle(label.closest('button')))).toBeLessThan(1)
  })
}


const shown = (element) => !!element && element.getClientRects().length > 0
// The labels that give way before the model name, in the order they go.
const otherLabels = (thread) => [thread.querySelector('.context-trigger > span'), ...['mode', 'thinking', 'machine'].map((menu) => thread.querySelector(`[data-menu="${menu}"] .label`))]

// Holds the band to the priority order: a label goes only after the ones
// before it, the model name is the last label to go, and the band collapses
// no control it has room for.
function expectPriority(thread) {
  const meta = thread.querySelector('.composer-meta')
  const name = thread.querySelector('[data-menu="model"] .label')
  const visible = otherLabels(thread).map(shown)
  for (let index = 1; index < visible.length; index++) if (!visible[index]) expect(visible[index - 1]).toBe(false)
  // The model name clips, or keeps its icon alone, only after every other label went.
  if (!shown(name) || name.scrollWidth > name.clientWidth) expect(visible.some(Boolean)).toBe(false)
  if (shown(name)) expect(name.clientWidth).toBeGreaterThanOrEqual(Math.min(name.scrollWidth, MIN_MODEL_NAME) - 0.5)
  // Until every control keeps its icon alone, none runs past the meta side.
  const level = Number(meta.dataset.collapse ?? 0)
  if (level < COLLAPSE_ORDER.length) for (const control of meta.children) expect(control.getBoundingClientRect().right).toBeLessThanOrEqual(meta.getBoundingClientRect().right + 0.5)
  // The last control the band collapsed has no room to show its label: shown
  // again, it runs past the band or clips the model name.
  if (!level) return
  const last = [...meta.querySelectorAll(COLLAPSE_ORDER[level - 1])]
  for (const control of last) delete control.dataset.collapsed
  const overflows = [...meta.children].some((control) => control.getBoundingClientRect().right > meta.getBoundingClientRect().right + 0.5)
  expect(overflows || name.clientWidth + 0.5 < name.scrollWidth).toBe(true)
  for (const control of last) control.dataset.collapsed = 'true'
}

// The thread beside the 195px sidebar and an open agent profile: the profile
// at its default share of the window, or widened as in the live window where
// the composer runs under 480px below a 1205px window.
const profiles = [['at its default width', (width) => width - 195 - Math.round(Math.min(560, Math.max(240, width * 0.28)))], ['widened', (width) => width - 677]]
for (const [profile, column] of profiles) {
  for (const width of [1280, 1100, 960]) {
    it(`keeps the model name the last label to go with the profile ${profile} in a ${width}px window`, async () => {
      await page.viewport(width, 800)
      const thread = composer(column(width), { model: 'Claude Sonnet 5.5', context: true })
      await document.fonts.ready
      fitComposerMeta(thread.querySelector('.composer-meta'))
      expectPriority(thread)
      // The default profile leaves room for the whole model name at each width.
      const name = thread.querySelector('[data-menu="model"] .label')
      if (profile === 'at its default width') expect(name.scrollWidth).toBeLessThanOrEqual(name.clientWidth)
    })
  }
}

// A model name wider than a short one clips only after the thinking level
// and the permission mode give way, at every thread width.
it('collapses Medium and Bypass permissions before "Claude Sonnet 5.5" clips', async () => {
  await page.viewport(1280, 800)
  for (let column = 1085; column >= 300; column -= 15) {
    const thread = composer(column, { model: 'Claude Sonnet 5.5', context: true })
    await document.fonts.ready
    fitComposerMeta(thread.querySelector('.composer-meta'))
    expectPriority(thread)
    const name = thread.querySelector('[data-menu="model"] .label')
    if (shown(thread.querySelector('[data-menu="thinking"] .label'))) expect(name.scrollWidth, `${column}px`).toBeLessThanOrEqual(name.clientWidth)
    for (const component of components.splice(0)) unmount(component)
    document.body.replaceChildren()
  }
})
