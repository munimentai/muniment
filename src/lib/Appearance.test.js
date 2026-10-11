import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/svelte'

import Appearance from './Appearance.svelte'

afterEach(() => {
  cleanup()
  localStorage.clear()
  delete document.documentElement.dataset.theme
  delete document.documentElement.dataset.scheme
  delete document.documentElement.dataset.frame
})

const stored = () => JSON.parse(localStorage.getItem('muniment.theme'))
const pressed = (group) => within(group).getAllByRole('button').filter((button) => button.getAttribute('aria-pressed') === 'true').map((button) => button.textContent.trim())

describe('Appearance', () => {
  it('offers three modes and four colors in each of three styles, each with its light and dark tiles', () => {
    render(Appearance)
    const modes = screen.getByRole('group', { name: 'Mode' })
    expect(within(modes).getAllByRole('button').map((button) => button.textContent)).toEqual(['System', 'Light', 'Dark'])
    const rows = ['Normal', 'Vibrant', 'High contrast'].map((style) => screen.getByRole('group', { name: `${style} color schemes` }))
    for (const row of rows) expect(within(row).getAllByRole('button').map((button) => button.textContent.trim())).toEqual(['Moss', 'Harbor', 'Clay', 'Iris'])
    expect(within(rows[1]).getAllByRole('button').map((button) => button.getAttribute('aria-label'))).toEqual(['Moss vibrant', 'Harbor vibrant', 'Clay vibrant', 'Iris vibrant'])
    expect([...rows[2].querySelectorAll('[data-swatch]')].map((swatch) => swatch.dataset.theme)).toEqual(['moss', 'harbor', 'clay', 'iris'].flatMap((color) => [`${color}-contrast-light`, `${color}-contrast-dark`]))
    // Every tile shows the tokens that differ between themes, and a vibrant tile its accent.
    for (const [index, row] of rows.entries()) {
      for (const swatch of row.querySelectorAll('[data-swatch]')) {
        expect([...swatch.querySelectorAll('i')].map((bar) => bar.classList[0])).toEqual(['swatch-ink', 'swatch-muted', 'swatch-signal', ...(index === 1 ? ['swatch-accent'] : [])])
      }
    }
    expect(pressed(modes)).toEqual(['Dark'])
    expect(rows.flatMap(pressed)).toEqual(['Moss'])
  })

  it('keeps a palette across modes, shows its light or dark version, and follows the OS in System', async () => {
    render(Appearance)
    const modes = screen.getByRole('group', { name: 'Mode' })
    const vibrant = screen.getByRole('group', { name: 'Vibrant color schemes' })
    await fireEvent.click(within(modes).getByRole('button', { name: 'System' }))
    await fireEvent.click(within(vibrant).getByRole('button', { name: 'Iris vibrant' }))
    expect(document.documentElement.dataset.theme).toBe('iris-vibrant-light')
    expect(document.documentElement.dataset.scheme).toBeUndefined()
    expect(stored()).toEqual({ mode: 'system', palette: 'iris-vibrant' })
    expect(pressed(vibrant)).toEqual(['Iris'])
    await fireEvent.click(within(modes).getByRole('button', { name: 'Dark' }))
    expect(document.documentElement.dataset).toMatchObject({ theme: 'iris-vibrant-dark', scheme: 'dark' })
    await fireEvent.click(within(modes).getByRole('button', { name: 'Light' }))
    expect(document.documentElement.dataset).toMatchObject({ theme: 'iris-vibrant-light', scheme: 'light' })
    expect(stored()).toEqual({ mode: 'light', palette: 'iris-vibrant' })
    await fireEvent.click(within(screen.getByRole('group', { name: 'Normal color schemes' })).getByRole('button', { name: 'Moss' }))
    await fireEvent.click(within(modes).getByRole('button', { name: 'System' }))
    expect(document.documentElement.dataset.theme).toBeUndefined()
    expect(document.documentElement.dataset.scheme).toBeUndefined()
    expect(stored()).toEqual({ mode: 'system', palette: 'moss' })
  })

  it('swaps the thinking indicator preview in place, one indicator at a time', async () => {
    render(Appearance)
    const preview = screen.getByLabelText('Thinking indicator preview')
    for (const name of ['Text sheen', 'Ear flap', 'Motion elephant']) {
      await fireEvent.click(screen.getByRole('button', { name }))
      expect(preview.querySelectorAll('.thinking')).toHaveLength(1)
    }
    expect(preview.querySelector('.walker')).not.toBeNull()
    expect(localStorage.getItem('muniment.thinking-mark')).toBe('motion')
  })

  it('picks Framed or Frameless and applies it at once beside the theme', async () => {
    render(Appearance)
    const frames = screen.getByRole('group', { name: 'Frame' })
    expect(within(frames).getAllByRole('button').map((button) => button.textContent.trim())).toEqual(['Framed', 'Frameless'])
    expect(pressed(frames)).toEqual(['Framed'])
    await fireEvent.click(within(frames).getByRole('button', { name: 'Frameless' }))
    expect(document.documentElement.dataset.frame).toBe('frameless')
    expect(localStorage.getItem('muniment.frame')).toBe('frameless')
    expect(pressed(frames)).toEqual(['Frameless'])
    await fireEvent.click(within(screen.getByRole('group', { name: 'Normal color schemes' })).getByRole('button', { name: 'Harbor' }))
    expect(document.documentElement.dataset).toMatchObject({ theme: 'harbor-dark', frame: 'frameless' })
  })
})
