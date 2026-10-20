import { afterEach, expect, it } from 'vitest'
import { createRawSnippet, flushSync, mount, unmount } from 'svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import MachinePicker from './MachinePicker.svelte'
import ComposerMenuButton from '../composer/ComposerMenuButton.svelte'

let components = []
afterEach(() => {
  for (const component of components) unmount(component)
  components = []
  document.body.replaceChildren()
})

const here = { id: 'local', name: 'This computer', kind: 'local', local: true, active: true, status: 'online' }
const pod = { id: 'pod', name: 'Pod', kind: 'container', status: 'offline' }
const list = { selected: 'local', runtimes: [here, pod] }
const glyph = (name) => createRawSnippet(() => ({ render: () => `<svg class="lucide" data-icon="${name}" width="14" height="14"></svg>` }))

// The composer band at `width`, a size container like the composer, with the
// machine chip of an open thread or a new one beside a model chip.
function band(width, props = {}) {
  const composer = document.createElement('div')
  composer.style.cssText = `container: composer / inline-size; width: ${width}px; display: flex; gap: 4px;`
  document.body.append(composer)
  components.push(mount(MachinePicker, { target: composer, props: { list, ...props } }))
  components.push(mount(ComposerMenuButton, { target: composer, props: { menu: 'model', label: 'Model: GPT-6 Astra', text: 'GPT-6 Astra', icon: glyph('openai'), compact: glyph('openai') } }))
  flushSync()
  return composer
}

const shows = (element) => !!element && element.checkVisibility()
const machineChip = (composer) => composer.querySelector('[data-menu="machine"]')
const modelChip = (composer) => composer.querySelector('[data-menu="model"]')

for (const [state, props] of [['a new thread', {}], ['an open thread', { locked: 'local' }]]) {
  it(`keeps the machine chip of ${state} to the picked machine's icon where the model chip keeps its icon`, () => {
    const narrow = band(400, props)
    expect(shows(modelChip(narrow).querySelector('.label'))).toBe(false)
    const chip = machineChip(narrow)
    expect(shows(chip.querySelector('.label'))).toBe(false)
    const icons = [...chip.querySelectorAll('[data-icon]')].filter(shows).map((icon) => icon.dataset.icon)
    expect(icons).toEqual(['monitor'])
    expect(chip.getAttribute('aria-label')).toMatch(/^Machine: This computer\./)
    expect(chip.dataset.tooltip).toBe(chip.getAttribute('aria-label'))
    expect(getComputedStyle(chip, '::after').display).toBe('block')
  })

  it(`shows the machine name of ${state} while the model chip shows its name`, () => {
    const wide = band(640, props)
    expect(shows(modelChip(wide).querySelector('.label'))).toBe(true)
    expect(shows(machineChip(wide).querySelector('.label'))).toBe(true)
    expect(machineChip(wide).querySelector('.label').textContent).toBe('This computer')
  })
}
