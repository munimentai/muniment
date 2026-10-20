import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ComposerNotice from './ComposerNotice.svelte'
import { createComposerNotices } from '../lib/composer-notices.js'

beforeEach(() => vi.useFakeTimers())
afterEach(() => {
  cleanup()
  vi.clearAllTimers()
  vi.useRealTimers()
})

const later = (ms) => act(() => vi.advanceTimersByTime(ms))

function mount() {
  const notices = createComposerNotices()
  const composer = document.createElement('div')
  composer.className = 'composer'
  composer.append(document.createElement('textarea'))
  document.body.append(composer)
  render(ComposerNotice, { target: composer, props: { notices } })
  return { notices, composer }
}

it('shows the current notice on the composer panel surface and leaves after six seconds', async () => {
  const { notices } = mount()
  await act(() => notices.show({ id: 'one', cause: 1, text: 'Scheduled agents run only while Muniment is open.', label: 'Background service' }))
  const notice = screen.getByRole('status', { name: 'Background service' })
  expect(notice).toHaveTextContent('Scheduled agents run only while Muniment is open.')
  expect(notice).toHaveAttribute('data-composer-panel')
  expect(notice).toHaveAttribute('data-panel-variant', 'overlay')
  expect(notice.querySelector('.record')).toBeNull()
  await later(5999)
  expect(screen.getByRole('status')).toBeInTheDocument()
  await later(1)
  expect(screen.queryByRole('status')).toBeNull()
})

it('pauses while hovered or holding keyboard focus', async () => {
  const { notices } = mount()
  await act(() => notices.show({ id: 'one', text: 'Moved.', action: { label: 'Open', run: () => {} } }))
  const notice = screen.getByRole('status')
  await fireEvent.pointerEnter(notice)
  await later(30_000)
  expect(screen.getByRole('status')).toBeInTheDocument()
  await fireEvent.pointerLeave(notice)
  await act(() => screen.getByRole('button', { name: 'Open' }).focus())
  await later(30_000)
  expect(screen.getByRole('status')).toBeInTheDocument()
  await act(() => screen.getByRole('button', { name: 'Open' }).blur())
  await later(6000)
  expect(screen.queryByRole('status')).toBeNull()
})

it('runs the action and closes on the close control, then shows the next notice', async () => {
  const { notices, composer } = mount()
  const run = vi.fn()
  const ondismiss = vi.fn()
  await act(() => {
    notices.show({ id: 'one', text: 'First.', action: { label: 'Open Login Items', run }, dismissLabel: 'Dismiss first', ondismiss })
    notices.show({ id: 'two', text: 'Second.' })
  })
  expect(screen.getAllByRole('status')).toHaveLength(1)
  await fireEvent.click(screen.getByRole('button', { name: 'Open Login Items' }))
  expect(run).toHaveBeenCalledOnce()
  expect(screen.getByRole('status')).toHaveTextContent('First.')
  const close = screen.getByRole('button', { name: 'Dismiss first' })
  close.focus()
  await fireEvent.click(close)
  expect(ondismiss).toHaveBeenCalledOnce()
  expect(screen.getByRole('status')).toHaveTextContent('Second.')
  await act(() => Promise.resolve())
  expect(composer.querySelector('textarea')).toHaveFocus()
})

it('waits under an open composer panel', async () => {
  const { notices, composer } = mount()
  const panel = document.createElement('div')
  panel.setAttribute('data-composer-panel', '')
  composer.append(panel)
  await act(() => notices.show({ id: 'one', text: 'Waiting.' }))
  await later(12_000)
  expect(screen.getByRole('status')).toHaveTextContent('Waiting.')
  panel.remove()
  await later(6000)
  expect(screen.queryByRole('status')).toBeNull()
})
