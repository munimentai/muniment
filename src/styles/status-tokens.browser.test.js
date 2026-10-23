import { afterEach, expect, it } from 'vitest'
import './tokens.css'

afterEach(() => {
  document.body.replaceChildren()
  delete document.documentElement.dataset.theme
})

const colors = ['moss', 'harbor', 'clay', 'iris']
const themes = colors.flatMap((color) => [color, `${color}-vibrant`, `${color}-contrast`]).flatMap((palette) => [`${palette}-light`, `${palette}-dark`])
const STATE = ['success', 'warning', 'danger', 'active'].flatMap((state) => [state, `${state}-soft`, `${state}-edge`])

// Each state token as the root's theme resolves it, read through a color
// property so light-dark() and color-mix() come out as one rgb value.
function resolved(theme) {
  document.documentElement.dataset.theme = theme
  const probe = document.createElement('i')
  document.body.append(probe)
  const read = (token) => { probe.style.color = `var(--${token})`; return getComputedStyle(probe).color }
  const values = Object.fromEntries([...STATE, 'accent'].map((token) => [token, read(token)]))
  probe.remove()
  return values
}

it('draws every state in the same color in every color scheme of a mode', () => {
  for (const mode of ['light', 'dark']) {
    const shown = themes.filter((theme) => theme.endsWith(`-${mode}`)).map((theme) => [theme, resolved(theme)])
    const [, first] = shown[0]
    for (const [theme, values] of shown) for (const token of STATE) expect(values[token], `${theme} --${token}`).toBe(first[token])
    // The schemes do differ: their accents are not one color.
    expect(new Set(shown.map(([, values]) => values.accent)).size).toBeGreaterThan(1)
  }
  expect(resolved('clay-light').success).not.toBe(resolved('clay-dark').success)
})
