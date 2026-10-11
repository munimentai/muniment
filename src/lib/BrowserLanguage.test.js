import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import BrowserLanguage from './BrowserLanguage.svelte'

const view = (selected = null, active = null, nl = 'available') => ({
  selected, active, downloadSize: 9_400_000,
  languages: [{ code: 'fr', state: 'bundled' }, { code: 'nl', state: nl }],
})

afterEach(cleanup)

describe('BrowserLanguage', () => {
  it('lists each language by name and marks the ones that download', async () => {
    const invoke = vi.fn(async () => view())
    render(BrowserLanguage, { tauri: { invoke } })
    await fireEvent.click(await screen.findByRole('button', { name: 'Language: Match system' }))
    expect(screen.getByRole('menuitemradio', { name: 'français (French)' })).toBeTruthy()
    expect(screen.getByRole('menuitemradio', { name: 'Nederlands (Dutch) · Download' })).toBeTruthy()
    expect(screen.getByText('Languages marked Download fetch about 9 MB once.')).toBeTruthy()
  })

  it('downloads a missing language and asks for a restart', async () => {
    let finish
    const invoke = vi.fn(async (command) => command === 'browser_language'
      ? view()
      : new Promise(resolve => { finish = () => resolve(view('nl', null, 'downloaded')) }))
    render(BrowserLanguage, { tauri: { invoke } })
    await fireEvent.click(await screen.findByRole('button', { name: 'Language: Match system' }))
    await fireEvent.click(screen.getByRole('menuitemradio', { name: 'Nederlands (Dutch) · Download' }))
    expect(invoke).toHaveBeenLastCalledWith('browser_language_set', { code: 'nl' })
    expect(await screen.findByText('Downloading the language…')).toBeTruthy()
    finish()
    expect(await screen.findByText('Restart Muniment to change the browser language.')).toBeTruthy()
    expect(screen.queryByText('Downloading the language…')).toBeNull()
  })

  it('returns to the system language and shows a failed download', async () => {
    const invoke = vi.fn(async (command, args) => {
      if (command === 'browser_language') return view('fr', 'fr')
      if (args.code === 'nl') throw 'The language download failed. Check your connection and try again.'
      return view(null, 'fr')
    })
    render(BrowserLanguage, { tauri: { invoke } })
    await fireEvent.click(await screen.findByRole('button', { name: 'Language: français (French)' }))
    await fireEvent.click(screen.getByRole('menuitemradio', { name: 'Nederlands (Dutch) · Download' }))
    expect((await screen.findByRole('alert')).textContent).toBe('The language download failed. Check your connection and try again.')
    await fireEvent.click(screen.getByRole('button', { name: 'Language: français (French)' }))
    await fireEvent.click(screen.getByRole('menuitemradio', { name: 'Match system' }))
    expect(invoke).toHaveBeenLastCalledWith('browser_language_set', { code: null })
    expect(await screen.findByText('Restart Muniment to change the browser language.')).toBeTruthy()
  })
})
