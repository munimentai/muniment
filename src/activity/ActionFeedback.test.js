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
