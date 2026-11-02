import { afterEach, expect, it } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { createRawSnippet, flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import appSource from '../App.svelte?raw'
import ChatComposer from './ChatComposer.svelte'
import ComposerMenuButton from './ComposerMenuButton.svelte'
import ContextMeter from '../lib/ContextMeter.svelte'
import { fitComposerMeta } from './composer-priority.js'

let components = []
afterEach(() => {
  for (const component of components) unmount(component)
  components = []
  document.body.replaceChildren()
})

// The composer band's own rules, as App.svelte writes them, applied to the
// same class names outside the component's scope.
const bandRules = [...appSource.match(/<style>([\s\S]*)<\/style>/)[1].replace(/\/\*[\s\S]*?\*\//g, '').matchAll(/([^{}]+)\{([^{}]*)\}/g)]
  .filter(([, selector]) => /^\s*\.composer-(row|meta|actions|icon|action)\b/.test(selector) && !selector.includes('@'))
  .map(([, selector, body]) => `${selector.replaceAll(/:global\(([^)]*)\)/g, '$1').trim()} {${body}}`)
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
for (const [state, column, clips] of [['alone', 1085, false], ['beside the agent profile', 765, false], ['beside a wider agent profile', 640, false], ['beside the widest agent profile', 560, true]]) {
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

