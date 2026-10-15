import '@testing-library/jest-dom/vitest'
import { cleanup, render, fireEvent } from '@testing-library/svelte'
import { afterEach, expect, it, vi } from 'vitest'
import ActionFeedback from './ActionFeedback.svelte'

afterEach(cleanup)

it('opens the group and action details and stops the active sheen when the run ends', async () => {
  const activities = [{ effectId: 'command-0', displayName: 'bash', status: 'completed', input: '{"command":"pwd"}' }, { effectId: 'command-1', displayName: 'bash', status: 'running', input: '{"command":"cargo test"}', output: '12 tests passed' }]
  const view = render(ActionFeedback, { activities, live: true })
  const tools = view.getByText('2 tools used').closest('details')
  expect(tools).not.toHaveAttribute('open')
  await fireEvent.click(tools.querySelector('summary'))
  expect(tools).toHaveAttribute('open')
  const group = view.getByText('Running commands').closest('details')
  expect(group).not.toHaveAttribute('open')
  await fireEvent.click(group.querySelector('summary'))
  expect(group).toHaveAttribute('open')
  const action = view.getByText('Running cargo test').closest('details')
  await fireEvent.click(action.querySelector('summary'))
  expect(action).toHaveAttribute('open')
  expect(view.getByText('12 tests passed')).toBeVisible()
  expect(view.container.querySelectorAll('.active-sheen')).toHaveLength(3)
  await view.rerender({ activities, live: false })
  expect(view.container.querySelector('.active-sheen')).toBeNull()
  expect(view.getAllByText('Interrupted')).toHaveLength(2)
})

it('leaves rows without details static and opens read files in the panel', async () => {
  const opened = []
  const view = render(ActionFeedback, { onopenfile: (file) => opened.push(file), activities: [
    { effectId: 'a', displayName: 'search', status: 'completed', input: '{"query":"renewals"}' },
    { effectId: 'b', displayName: 'read', status: 'completed', input: '{"path":"src/main.rs"}' },
  ] })
  expect(view.getByText('Searched renewals').closest('.plain-action')).not.toBeNull()
  await fireEvent.click(view.getByRole('button', { name: 'Read main.rs', hidden: true }))
  expect(opened).toEqual([{ path: 'src/main.rs' }])
})

it('shows only the queries for web search details', () => {
  const view = render(ActionFeedback, { activities: [{ effectId: 'web', displayName: 'web_search', status: 'completed', input: JSON.stringify({ queries: ['First query', 'Second query'], numResults: 6 }), output: 'Long provider output' }] })
  expect(view.getAllByText('Searched the web')).toHaveLength(1)
  expect(view.getByText('1 tool used')).toBeInTheDocument()
  expect(view.container.querySelectorAll('details')).toHaveLength(2)
  expect(view.getByText('Queries:')).toBeInTheDocument()
  expect(view.getByText('First query')).toBeInTheDocument()
  expect(view.queryByText('Long provider output')).not.toBeInTheDocument()
  expect(view.container.textContent).not.toContain('Num Results')
  expect(view.container.textContent).not.toContain('Completed')
})

it('reads the whole output of a truncated action when its detail opens', async () => {
  const reads = []
  let answer
  const loadoutput = (effectId) => {
    reads.push(effectId)
    return new Promise((resolve, reject) => { answer = { resolve, reject } })
  }
  const activities = [
    { effectId: 'log', displayName: 'bash', status: 'completed', input: '{"command":"cat build.log"}', output: 'first lines', outputBytes: 400_000, outputTruncated: true },
    { effectId: 'short', displayName: 'bash', status: 'completed', input: '{"command":"pwd"}', output: '/work', outputBytes: 5 },
  ]
  const view = render(ActionFeedback, { activities, loadoutput })
  await fireEvent.click(view.getByText('2 tools used').closest('summary'))
  await fireEvent.click(view.getByText('Ran commands').closest('summary'))
  await fireEvent.click(view.getByText('Ran pwd').closest('summary'))
  const action = view.getByText('Ran cat build.log').closest('details')
  await fireEvent.click(action.querySelector('summary'))
  await vi.waitFor(() => expect(reads).toEqual(['log']))
  expect(view.getByText('first lines')).toBeInTheDocument()
  expect(view.getByText('Loading the full output.')).toBeInTheDocument()

  answer.reject(new Error('unavailable'))
  const retry = await view.findByRole('button', { name: 'Load full output' })
  expect(view.getByRole('alert')).toHaveTextContent('The full output could not be loaded.')
  await fireEvent.click(retry)
  expect(reads).toEqual(['log', 'log'])
  answer.resolve({ kind: 'tool_output', text: 'all of the build log', bytes: 400_000, truncated: true })
  expect(await view.findByText('all of the build log')).toBeInTheDocument()
  expect(view.getByText('Showing the first 20 B of 391 KB.')).toBeInTheDocument()
})
