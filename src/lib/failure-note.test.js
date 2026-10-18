import { afterEach, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import FailureNote from './FailureNote.svelte'
import { failureLine, failureLog } from './failure-note.js'

afterEach(cleanup)

it('shows the first sentence and keeps the rest and the cause behind Show log', () => {
  expect(failureLine('The machine was not added\nconnect: refused')).toBe('The machine was not added.')
  expect(failureLog('The machine was not added.\nconnect: refused', 'tls: bad certificate')).toEqual(['connect: refused', 'tls: bad certificate'])
  expect(failureLog('Reply failed.', 'Reply failed.')).toEqual([])
  render(FailureNote, { failure: 'The update failed.\nexit status 3', detail: 'disk full' })
  expect(screen.getByRole('alert')).toHaveTextContent('The update failed.')
  expect(screen.getByText('Show log')).toBeInTheDocument()
  expect(screen.getByText('exit status 3')).toBeInTheDocument()
  expect(screen.getByText('disk full')).toBeInTheDocument()
})

it('shows no log when the failure says everything in one line', () => {
  render(FailureNote, { failure: 'The phone could not be revoked.' })
  expect(screen.queryByText('Show log')).toBeNull()
})
