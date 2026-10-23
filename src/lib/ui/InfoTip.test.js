import { afterEach, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import PageHead from './PageHead.svelte'
afterEach(cleanup)

it('titles a page and keeps its explanation behind an info control that describes the title', async () => {
  render(PageHead, { props: { title: 'Search', intro: 'How agents search the web.' } })
  const heading = screen.getByRole('heading', { name: 'Search', level: 3 })
  expect(heading).toHaveAccessibleDescription('How agents search the web.')
  const info = screen.getByRole('button', { name: 'About Search' })
  expect(info).toHaveAttribute('aria-expanded', 'false')
  await fireEvent.click(info)
  expect(info).toHaveAttribute('aria-expanded', 'true')
  await fireEvent.keyDown(info, { key: 'Escape' })
  expect(info).toHaveAttribute('aria-expanded', 'false')
})

it('keeps the plain intro when a page has no title', () => {
  render(PageHead, { props: { intro: 'Threads in this project.' } })
  expect(screen.getByText('Threads in this project.')).toHaveClass('settings-intro')
  expect(screen.queryByRole('heading')).toBeNull()
})
