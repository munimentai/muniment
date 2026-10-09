import { afterEach, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import BrowserSearch from './BrowserSearch.svelte'

afterEach(() => { cleanup(); localStorage.clear() })

it('chooses the address bar search engine and names the bangs', async () => {
  render(BrowserSearch)
  await fireEvent.click(screen.getByRole('button', { name: 'Search engine: Google' }))
  await fireEvent.click(screen.getByRole('menuitemradio', { name: 'Brave Search' }))
  expect(localStorage.getItem('muniment.browser.search')).toBe('brave')
  expect(screen.getByRole('button', { name: 'Search engine: Brave Search' })).toBeInTheDocument()
  expect(screen.getByText(/!w for Wikipedia/)).toBeInTheDocument()
})
