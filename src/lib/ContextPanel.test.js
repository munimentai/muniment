import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ContextPanel from './ContextPanel.svelte'
import ContextMeter from './ContextMeter.svelte'
afterEach(cleanup)

const usage = (fields = {}) => ({ tokens: 46210, contextWindow: 200000, percent: 23.1, estimated: false, windowKnown: true, provider: 'anthropic', model: 'claude-sonnet', autoCompact: true, compactAt: 183616, compacting: false, ...fields })

it('shows the count, the compaction point and compacts', async () => {
  const oncompact = vi.fn()
  render(ContextPanel, { usage: usage(), oncompact, onsettings: vi.fn(), onclose: vi.fn() })
  expect(screen.getByText('46,210')).toBeInTheDocument()
  expect(screen.getByText('Compacts automatically at 183,616 tokens.')).toBeInTheDocument()
  expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
  await fireEvent.click(screen.getByRole('button', { name: 'Compact now' }))
  expect(oncompact).toHaveBeenCalledWith(null)
})

it('waits for a reply in flight and names an unknown window', async () => {
  const oncompact = vi.fn()
  render(ContextPanel, { usage: usage({ windowKnown: false, contextWindow: 128000, model: 'qwen3:8b' }), active: true, oncompact, onsettings: vi.fn(), onclose: vi.fn() })
  expect(screen.getByText('The model reports no context window for qwen3:8b. Its server may drop older messages without warning.')).toBeInTheDocument()
  expect(screen.getByRole('button', { name: 'Compact now' })).toBeDisabled()
  expect(screen.getByRole('status')).toHaveTextContent('Compact after this reply finishes.')
})

it('opens Context settings and shows a compaction error', async () => {
  const onsettings = vi.fn(), onclose = vi.fn()
  render(ContextPanel, { usage: usage({ autoCompact: false }), error: 'The summary could not finish.', oncompact: vi.fn(), onsettings, onclose })
  expect(screen.getByText('Automatic compaction is off.')).toBeInTheDocument()
  expect(screen.getByRole('alert')).toHaveTextContent('The summary could not finish.')
  await fireEvent.click(screen.getByRole('button', { name: 'Context settings' }))
  expect(onclose).toHaveBeenCalledOnce()
  expect(onsettings).toHaveBeenCalledOnce()
})

it('renders the meter level and its hover label', () => {
  render(ContextMeter, { usage: usage({ tokens: 170000, percent: 85 }), onclick: vi.fn() })
  const meter = screen.getByRole('button', { name: 'Context, 170,000 of 200,000 tokens' })
  expect(meter).toHaveAttribute('data-level', 'caution')
  expect(meter).toHaveTextContent('85%')
})
