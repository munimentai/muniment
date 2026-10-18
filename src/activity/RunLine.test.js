import '@testing-library/jest-dom/vitest'
import { cleanup, fireEvent, render } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import RunLine from './RunLine.svelte'

afterEach(cleanup)

const run = {
  id: 'run-1',
  phase: 'complete',
  receipt: { model: 'openai/gpt-5.5', time: '6.2s', tokens: { input: 900, output: 100 }, cost_usd: 0.014, permission_mode: 'manual' },
  toolActivity: [{ effectId: 'a', displayName: 'bash', status: 'failed', input: '{"command":"npm test"}', startedAt: '2026-10-18T12:00:00Z', finishedAt: '2026-10-18T12:00:04Z' }],
}

it('opens a turn with one line and expands to its tool calls and Review changes', async () => {
  const onreview = vi.fn()
  const view = render(RunLine, { run, machine: 'Studio', modes: [{ id: 'manual', title: 'Ask before changes' }], onreview })
  const line = view.getByTestId('run-line')
  expect(line.querySelector('summary')).toHaveTextContent('Studio·OpenAI · GPT 5.5·Ask before changes·6s·1k tokens·$0.01 API estimate')
  await fireEvent.click(line.querySelector('summary'))
  expect(line).toHaveAttribute('open')
  expect(view.getByRole('list', { name: 'Tool calls' })).toHaveTextContent('Ran npm testFailed4s')
  await fireEvent.click(view.getByRole('button', { name: 'Review changes' }))
  expect(onreview).toHaveBeenCalledOnce()
})

it('is a plain line when the turn has nothing to expand, and shows nothing without a record', () => {
  const view = render(RunLine, { run: { ...run, toolActivity: [] }, machine: 'This computer' })
  expect(view.getByTestId('run-line').tagName).toBe('P')
  cleanup()
  const empty = render(RunLine, { run: { phase: 'complete', receipt: null, toolActivity: [] } })
  expect(empty.queryByTestId('run-line')).toBeNull()
})
