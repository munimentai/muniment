import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import { approvalChoices, approvalOutcome, approvalSummary, composerGateOptions, isPlanGate, longInput, ruleInputs, threadApprovals } from './approvals.js'

const fixture = (name) => JSON.parse(fs.readFileSync(path.join(process.cwd(), 'core/protocol-fixtures/muniment.attach/1', name), 'utf8')).body
const pending = fixture('response-agent-approvals.json').approvals[0]

describe('approval answers', () => {
  it('offers exactly the answers core lists, in a fixed order', () => {
    expect(approvalChoices(pending).map((choice) => [choice.label, choice.answer])).toEqual([
      ['Allow once', 'allow_once'],
      ['Allow for this thread', 'allow_thread'],
      ['Always allow for this agent', 'always_allow_agent'],
      ['Deny', 'deny'],
    ])
    expect(approvalChoices(pending).at(-1).tone).toBe('deny')
  })

  it('leaves out Always allow when an ask-first rule matches', () => {
    const limited = { ...pending, answers: ['deny', 'allow_thread', 'allow_once'] }
    expect(approvalChoices(limited).map((choice) => choice.answer)).toEqual(['allow_once', 'allow_thread', 'deny'])
  })

  it('answers an older runtime\'s gate with its own options', () => {
    const request = { kind: 'select', title: 'Allow bash? A write needs a yes.', options: ['Allow once', 'Allow for this thread', 'Deny'] }
    expect(approvalChoices({ request })).toEqual([
      { label: 'Allow once', answer: { type: 'select', value: 'Allow once' } },
      { label: 'Allow for this thread', answer: { type: 'select', value: 'Allow for this thread' } },
      { label: 'Deny', answer: { type: 'select', value: 'Deny' }, tone: 'deny' },
    ])
    expect(approvalChoices({ request: { kind: 'confirm', title: 'Allow bash?', message: 'git push' } }).map((choice) => choice.answer)).toEqual([
      { type: 'confirm', value: true },
      { type: 'confirm', value: false },
    ])
    expect(approvalChoices({ request: { kind: 'select', title: 'Pick', options: ['Red', 'Blue'] } }).map((choice) => choice.label)).toEqual(['Red', 'Blue'])
    expect(approvalChoices({ request: { kind: 'input', title: 'Name?' } })).toEqual([])
    expect(approvalChoices(undefined)).toEqual([])
  })
})

describe('approval summary', () => {
  it('names the exact operation and its inputs from the rule', () => {
    expect(approvalSummary(pending)).toEqual({
      operation: 'bash `git push origin main` in /work/agents/Lead-scout',
      title: pending.request.title,
      rows: [['Command', 'git push origin main'], ['Shell', 'bash'], ['Folder', '/work/agents/Lead-scout']],
      exact: true,
    })
  })

  it('reads a path or tool rule and an older gate', () => {
    expect(ruleInputs({ request_kind: 'path', resource: { type: 'path', path: '/tmp/a.md', operation: 'write' } })).toEqual([['Action', 'write'], ['Path', '/tmp/a.md']])
    expect(ruleInputs({ request_kind: 'tool', resource: { type: 'tool', name: 'mcp__crm__delete_lead', input: { id: 'lead-1' } } })).toEqual([['Tool', 'mcp__crm__delete_lead'], ['Input', '{\n  "id": "lead-1"\n}']])
    expect(ruleInputs({ request_kind: 'tool', resource: { type: 'tool', name: 'web_search', input: {} } })).toEqual([['Tool', 'web_search']])
    expect(approvalSummary({ request: { kind: 'confirm', title: 'Allow bash?', message: 'git push origin main' } })).toEqual({ operation: 'Allow bash?', title: 'Allow bash?', rows: [['Input', 'git push origin main']], exact: false })
    expect(approvalSummary({})).toEqual({ operation: 'Approval needed', title: 'Approval needed', rows: [], exact: false })
  })

  it('folds long inputs', () => {
    expect(longInput('git status')).toBe(false)
    expect(longInput('x'.repeat(300))).toBe(true)
    expect(longInput('a\nb\nc\nd\ne\nf\ng')).toBe(true)
  })

  it('lists one thread\'s pending approvals', () => {
    expect(threadApprovals([pending, { ...pending, approval_id: 'b', thread_id: 'other' }], pending.thread_id)).toEqual([pending])
    expect(threadApprovals([pending], null)).toEqual([])
  })
})

it('says when another device answered first or the answer is on its way', () => {
  expect(approvalOutcome('answered')).toBe('')
  expect(approvalOutcome('already_answered')).toBe('Another device answered this first. Its answer stands.')
  expect(approvalOutcome('sent')).toBe('Answer sent. The machine applies it when it answers.')
})

it('shows a composer gate without the agent-only answer and answers Deny with its option', () => {
  const gate = { kind: 'select', title: 'Allow bash `rm old.csv`?', options: ['Allow once', 'Allow for this thread', 'Always allow for this agent', 'Deny'] }
  expect(composerGateOptions(gate)).toEqual({ options: ['Allow once', 'Allow for this thread'], deny: { type: 'select', value: 'Deny' } })
  expect(composerGateOptions(gate, true).options).toContain('Always allow for this agent')
  // A thread without an agent offers the global answer core lists for it.
  const plain = { ...gate, options: ['Allow once', 'Allow for this thread', 'Always allow', 'Deny'] }
  expect(composerGateOptions(plain).options).toEqual(['Allow once', 'Allow for this thread', 'Always allow'])
  expect(composerGateOptions({ kind: 'select', title: 'Export target', options: ['PDF', 'Word'] })).toEqual({ options: ['PDF', 'Word'], deny: { type: 'cancelled' } })
  expect(isPlanGate({ kind: 'select', title: 'Approve this plan?', options: [] })).toBe(true)
  expect(isPlanGate(gate)).toBe(false)
})
