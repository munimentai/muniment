import { afterEach, expect, it } from 'vitest'
import { createRawSnippet, flushSync, mount, unmount } from 'svelte'
import { cleanup, render } from '@testing-library/svelte'
import '../styles/tokens.css'
import '../styles/base.css'
import '../styles/panels.css'
import ChatComposer from './ChatComposer.svelte'
import VoiceWave from './VoiceWave.svelte'
import { parseColor } from './voice-wave.js'

let components = []
afterEach(() => {
  delete document.documentElement.dataset.theme
  delete document.documentElement.dataset.motion
  cleanup()
  for (const component of components.reverse()) unmount(component)
  components = []
  document.body.replaceChildren()
})

const field = createRawSnippet(() => ({ render: () => '<textarea rows="2"></textarea>' }))
const until = async (check, ms = 4000) => {
  const end = performance.now() + ms
  while (!check()) {
    if (performance.now() > end) throw new Error('timed out')
    await new Promise((resolve) => setTimeout(resolve, 30))
  }
}

// The composer with the wave on it and a level source that speaks at `level`.
function composerWithWave() {
  const host = document.createElement('div')
  host.style.cssText = 'padding-top: 200px;'
  document.body.append(host)
  components.push(mount(ChatComposer, { target: host, props: { children: field } }))
  const composer = host.querySelector('.composer')
  const listeners = []
  const view = render(VoiceWave, { target: composer, props: { active: false, listen: async (_, callback) => { listeners.push(callback); return () => {} } } })
  const speak = (level) => { for (const listener of listeners) listener({ payload: { type: 'level', level } }) }
  return { composer, canvas: composer.querySelector('canvas'), view, speak }
}

function painted(canvas) {
  const data = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data
  let count = 0
  for (let i = 3; i < data.length; i += 4) if (data[i] > 0) count += 1
  return count
}

// The most opaque pixel in the column at `x` CSS pixels.
function column(canvas, x) {
  const ratio = canvas.width / canvas.clientWidth
  const data = canvas.getContext('2d').getImageData(Math.round(x * ratio), 0, 1, canvas.height).data
  let best = [0, 0, 0, 0]
  for (let i = 0; i < data.length; i += 4) if (data[i + 3] > best[3]) best = [...data.slice(i, i + 4)]
  return best
}

it.each([['moss-dark', '#58B39F'], ['clay-light', null]])('draws the wave over the top border in the %s accent and clears it once dictation stops', async (theme, expected) => {
  document.documentElement.dataset.theme = theme
  // The wave animates under reduced motion too: it is the live reading of the voice.
  document.documentElement.dataset.motion = 'reduce'
  const { composer, canvas, view, speak } = composerWithWave()
  const accent = getComputedStyle(document.documentElement).getPropertyValue('--accent').trim()
  if (expected) expect(accent).toBe(expected)
  const frame = composer.getBoundingClientRect()
  const box = canvas.getBoundingClientRect()
  expect(Math.round(box.left)).toBe(Math.round(frame.left))
  expect(Math.round(box.width)).toBe(Math.round(frame.width))
  expect(box.top + box.height / 2).toBeCloseTo(frame.top + 0.5, 0)
  expect(painted(canvas)).toBe(0)

  await view.rerender({ active: true })
  const timer = setInterval(() => speak(0.8), 33)
  try {
    await until(() => painted(canvas) > 200)
    expect(canvas.width).toBe(Math.round(canvas.clientWidth * devicePixelRatio))
    // A quarter of the way along, the line is the accent; at the corner it is the border.
    const [r, g, b] = column(canvas, frame.width * 0.25)
    const [ar, ag, ab] = parseColor(accent)
    expect(Math.abs(r - ar) + Math.abs(g - ag) + Math.abs(b - ab)).toBeLessThan(60)
  } finally {
    clearInterval(timer)
  }
  await view.rerender({ active: false })
  await until(() => painted(canvas) === 0, 8000)
})
