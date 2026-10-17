import fs from 'node:fs'
import path from 'node:path'
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/svelte'
import '@testing-library/jest-dom/vitest'
import ApprovalCard from './ApprovalCard.svelte'
afterEach(cleanup)

const fixture = (name) => JSON.parse(fs.readFileSync(path.join(process.cwd(), 'core/protocol-fixtures/muniment.attach/1', name), 'utf8')).body
const pending = { ...fixture('response-agent-approvals.json').approvals[0], runtimeId: 'box' }
const buttons = () => screen.getAllByRole('button').map((button) => button.textContent.trim())

it('shows the exact operation, its inputs and every answer core accepts', async () => {
  const invoke = vi.fn(async () => ({ status: 'answered', approval: { ...pending, state: 'answered' } }))
  const onanswered = vi.fn()
  render(ApprovalCard, { tauri: { invoke }, approval: pending, activity: { state: 'waiting', action: pending.summary }, onanswered })
  const card = screen.getByRole('article', { name: 'Approval for Lead scout' })
  expect(card).toHaveTextContent('bash `git push origin main` in /work/agents/Lead-scout')
  expect(card).toHaveTextContent('Commandgit push origin main')
  expect(card).toHaveTextContent('Folder/work/agents/Lead-scout')
  expect(buttons()).toEqual(['Allow once', 'Allow for this thread', 'Always allow for this agent', 'Deny'])
  expect(card.querySelector('.agent-avatar')).toHaveAttribute('data-activity', 'waiting')
  await fireEvent.click(screen.getByRole('button', { name: 'Allow for this thread' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('agent_request', { runtime: 'box', operation: 'agent.approval_answer', body: { approval_id: pending.approval_id, answer: 'allow_thread' } }))
  expect(onanswered).toHaveBeenCalledWith(pending, { status: 'answered', approval: { ...pending, state: 'answered' } }, '')
})

it('leaves out Always allow when an ask-first rule matches, and settles when another device answered', async () => {
  const invoke = vi.fn(async () => ({ status: 'already_answered', approval: null }))
  render(ApprovalCard, { tauri: { invoke }, approval: { ...pending, answers: ['allow_once', 'allow_thread', 'deny'] } })
  expect(buttons()).toEqual(['Allow once', 'Allow for this thread', 'Deny'])
  await fireEvent.click(screen.getByRole('button', { name: 'Deny' }))
  expect(await screen.findByRole('status')).toHaveTextContent('Another device answered this first. Its answer stands.')
  expect(screen.queryAllByRole('button')).toEqual([])
})

it('folds a long input behind Show all and names a refusal', async () => {
  const input = Object.fromEntries(Array.from({ length: 12 }, (_, index) => [`field${index}`, 'x'.repeat(30)]))
  const approval = { ...pending, summary: 'mcp__crm__update_lead', rule: { request_kind: 'tool', resource: { type: 'tool', name: 'mcp__crm__update_lead', input } } }
  const invoke = vi.fn(async () => { throw new Error('That machine is offline. Try again when it answers.') })
  const { container } = render(ApprovalCard, { tauri: { invoke }, approval })
  expect(container.querySelector('.inputs')).toHaveClass('folded')
  await fireEvent.click(screen.getByRole('button', { name: 'Show all' }))
  expect(container.querySelector('.inputs')).not.toHaveClass('folded')
  expect(screen.getByRole('button', { name: 'Show less' })).toHaveAttribute('aria-expanded', 'true')
  await fireEvent.click(screen.getByRole('button', { name: 'Allow once' }))
  expect(await screen.findByRole('status')).toHaveTextContent('That machine is offline. Try again when it answers.')
})
