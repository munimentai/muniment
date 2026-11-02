import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ProjectItem from './ProjectItem.svelte'

afterEach(cleanup)

it('uses the project name to toggle its threads and the pencil to start one', async () => {
  const ontoggle = vi.fn(), onnew = vi.fn()
  const { rerender } = render(ProjectItem, { name: 'Research', expanded: false, ontoggle, onnew })
  const title = screen.getByRole('button', { name: 'Project Research' })
  expect(title).toHaveAttribute('aria-expanded', 'false')
  await fireEvent.click(title)
  expect(ontoggle).toHaveBeenCalledOnce()
  await fireEvent.click(screen.getByRole('button', { name: 'New thread in Research' }))
  expect(onnew).toHaveBeenCalledOnce()
  await rerender({ expanded: true })
  expect(title).toHaveAttribute('aria-expanded', 'true')
  expect(title.querySelector('[data-icon="folder-open"]')).not.toBeNull()
})

it('states the triggers of its agents in words and names them with the project', () => {
  render(ProjectItem, { name: 'Homelab', triggers: 'every day 9:00', expanded: false, ontoggle: vi.fn(), onnew: vi.fn() })
  const title = screen.getByRole('button', { name: 'Project Homelab, every day 9:00' })
  expect(title).toHaveTextContent('every day 9:00')
  expect(title.querySelector('[data-icon="folder"]')).not.toBeNull()
  expect(title.querySelector('[data-icon="bot"]')).toBeNull()
})

it('offers only the actions the project has in its menu', async () => {
  const onsettings = vi.fn(), onrename = vi.fn()
  render(ProjectItem, { name: 'Research', expanded: false, ontoggle: vi.fn(), onnew: vi.fn(), onsettings, onrename })
  await fireEvent.click(screen.getByRole('button', { name: 'Research actions' }))
  expect(screen.getAllByRole('menuitem').map((item) => item.textContent.trim())).toEqual(['New thread', 'Settings', 'Rename'])
  await fireEvent.click(screen.getByRole('menuitem', { name: 'Settings' }))
  expect(onsettings).toHaveBeenCalledOnce()
})

it('moves focus into and out of its menu without scrolling the sidebar', async () => {
  const focus = vi.spyOn(HTMLElement.prototype, 'focus')
  try {
    render(ProjectItem, { name: 'Research', expanded: false, ontoggle: vi.fn(), onnew: vi.fn(), onrename: vi.fn() })
    await fireEvent.contextMenu(screen.getByRole('button', { name: 'Project Research' }), { clientX: 10, clientY: 10 })
    const first = await screen.findByRole('menuitem', { name: 'New thread' })
    expect(first).toHaveFocus()
    await fireEvent.keyDown(first, { key: 'Escape' })
    expect(screen.queryByRole('menu')).toBeNull()
    expect(focus.mock.calls.length).toBeGreaterThanOrEqual(2)
    for (const [options] of focus.mock.calls) expect(options).toEqual({ preventScroll: true })
  } finally { focus.mockRestore() }
})

it('shows the most urgent status of its threads in words', () => {
  render(ProjectItem, { name: 'Homelab', status: 'needs_you', statusLabel: 'Approve', expanded: false, ontoggle: vi.fn(), onnew: vi.fn() })
  const title = screen.getByRole('button', { name: 'Project Homelab, Approve' })
  expect(title).toHaveTextContent('Approve')
})
