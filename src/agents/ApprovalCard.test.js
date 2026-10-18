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
  expect(card.querySelector('.tool-title')).toHaveTextContent('Run git push origin main in Lead-scout')
  expect(card.querySelector('.tool-title code')).toHaveTextContent('git push origin main')
  expect(card.querySelector('.tool-details')).toHaveTextContent('Folder/work/agents/Lead-scout')
  expect(card.querySelector('.tool-raw')).not.toHaveAttribute('open')
  expect(card.querySelector('.tool-raw')).toHaveTextContent('"command": "git push origin main"')
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
  // An approval from a runtime that sends no presentation lists the rule's inputs.
  const approval = { ...pending, presentation: undefined, summary: 'mcp__crm__update_lead', rule: { request_kind: 'tool', resource: { type: 'tool', name: 'mcp__crm__update_lead', input } } }
  const invoke = vi.fn(async () => { throw new Error('That machine is offline. Try again when it answers.') })
  const { container } = render(ApprovalCard, { tauri: { invoke }, approval })
  expect(container.querySelector('.inputs')).toHaveClass('folded')
  await fireEvent.click(screen.getByRole('button', { name: 'Show all' }))
  expect(container.querySelector('.inputs')).not.toHaveClass('folded')
  expect(screen.getByRole('button', { name: 'Show less' })).toHaveAttribute('aria-expanded', 'true')
  await fireEvent.click(screen.getByRole('button', { name: 'Allow once' }))
  expect(await screen.findByRole('status')).toHaveTextContent('That machine is offline. Try again when it answers.')
})

it('shows why the gate asks', () => {
  render(ApprovalCard, { tauri: { invoke: vi.fn() }, approval: pending })
  expect(pending.reason).toBeTruthy()
  expect(screen.getByRole('article')).toHaveTextContent(pending.reason)
})

it('shows a plan with the four plan answers and approves it into a mode', async () => {
  const plan = { ...JSON.parse(fs.readFileSync(path.join(process.cwd(), 'core/protocol-fixtures/muniment.attach/1/event-agent-approval-pending-plan.json'), 'utf8')).body, runtimeId: 'box' }
  const invoke = vi.fn(async () => ({ status: 'answered', approval: { ...plan, state: 'answered' } }))
  render(ApprovalCard, { tauri: { invoke }, approval: plan })
  expect(screen.getByRole('region', { name: 'Plan' })).toHaveTextContent('Add a `--dry-run` flag to the export script.')
  expect(screen.getByRole('article')).toHaveTextContent('Approve this plan?')
  expect(screen.getByRole('article')).not.toHaveTextContent('plan_submit')
  expect(buttons()).toEqual(['Approve, then ask before changes', 'Approve and edit automatically', 'Approve with auto review', 'Keep planning'])
  await fireEvent.click(screen.getByRole('button', { name: 'Approve and edit automatically' }))
  await waitFor(() => expect(invoke).toHaveBeenCalledWith('agent_request', { runtime: 'box', operation: 'agent.approval_answer', body: { approval_id: plan.approval_id, answer: 'plan_edit_auto' } }))
})

const gallery = JSON.parse(fs.readFileSync(path.join(process.cwd(), 'core/protocol-fixtures/tool-presentation/presented.json'), 'utf8'))

it('leads with core\'s headline for any tool and keeps raw JSON behind Show details', () => {
  for (const gate of gallery.gates) {
    const approval = { ...pending, approval_id: gate.id, agent_name: gate.agent_name, summary: gate.summary, rule: gate.rule, reason: gate.reason, presentation: gate.presentation }
    const { container, unmount } = render(ApprovalCard, { tauri: { invoke: vi.fn() }, approval })
    expect(container.querySelector('.tool-title').textContent).toBe(gate.presentation.headline.replaceAll('`', ''))
    if (gate.reason) expect(container.querySelector('.tool-reason')).toHaveTextContent(gate.reason)
    const shown = container.cloneNode(true)
    shown.querySelector('.tool-raw')?.remove()
    expect(shown.textContent, gate.id).not.toMatch(/[{[]"|":/)
    expect(container.querySelector('.tool-raw').textContent).toMatch(/"tool"|"command"/)
    unmount()
  }
  const web = gallery.gates.find((gate) => gate.id === 'gate-web')
  render(ApprovalCard, { tauri: { invoke: vi.fn() }, approval: { ...pending, rule: web.rule, presentation: web.presentation, reason: web.reason } })
  expect(screen.getByRole('article')).toHaveTextContent('Search the web for \u201cLocal desktop October 2026\u201d and 1 more on news.ycombinator.com')
  expect(screen.getByRole('article').querySelector('.tool-details')).toHaveTextContent('Results per query8')
})
