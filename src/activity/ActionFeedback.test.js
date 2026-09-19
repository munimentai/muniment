import '@testing-library/jest-dom/vitest'
import { cleanup, render, fireEvent } from '@testing-library/svelte'
import { afterEach, expect, it } from 'vitest'
import ActionFeedback from './ActionFeedback.svelte'

afterEach(cleanup)

it('opens the group and action details and stops the active sheen when the run ends', async () => {
  const activities = [{ effectId: 'command-1', displayName: 'bash', status: 'running', input: '{"command":"cargo test"}', output: '12 tests passed' }]
  const view = render(ActionFeedback, { activities, live: true })
  const group = view.getByText('Running commands').closest('details')
  expect(group).not.toHaveAttribute('open')
  await fireEvent.click(group.querySelector('summary'))
  expect(group).toHaveAttribute('open')
  const action = view.getByText('Running cargo test').closest('details')
  await fireEvent.click(action.querySelector('summary'))
  expect(action).toHaveAttribute('open')
  expect(view.getByText('12 tests passed')).toBeVisible()
  expect(view.container.querySelectorAll('.active-sheen')).toHaveLength(2)
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
  expect(view.getAllByText('Searched the web')).toHaveLength(2)
  expect(view.getByText('Queries:')).toBeInTheDocument()
  expect(view.getByText('First query')).toBeInTheDocument()
  expect(view.queryByText('Long provider output')).not.toBeInTheDocument()
  expect(view.container.textContent).not.toContain('Num Results')
  expect(view.container.textContent).not.toContain('Completed')
})
