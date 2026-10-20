import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import Toggle from './Toggle.svelte'
afterEach(cleanup)

it('draws the small switch only on request and turns it on and off', async () => {
  const onchange = vi.fn()
  render(Toggle, { label: 'Briefing on', size: 'small', onchange })
  const small = screen.getByRole('switch', { name: 'Briefing on' })
  expect(small).toHaveClass('small')
  await fireEvent.click(small)
  expect(onchange).toHaveBeenCalledWith(true)
  expect(small).toHaveAttribute('aria-checked', 'true')
  cleanup()
  render(Toggle, { label: 'Use routing' })
  expect(screen.getByRole('switch', { name: 'Use routing' })).not.toHaveClass('small')
})
