import { expect, it } from 'vitest'
import { approvalChoices, approvalOutcome, approvalSummary } from './approvals.js'

it('offers Allow once, Always allow and Deny for an agent tool gate', () => {
  const request = { kind: 'select', title: 'Allow bash? A write needs a yes.', options: ['Allow once', 'Allow for this thread', 'Deny'] }
  expect(approvalChoices(request)).toEqual([
    { label: 'Allow once', answer: { type: 'select', value: 'Allow once' } },
    { label: 'Always allow', answer: { type: 'select', value: 'Allow for this thread' } },
    { label: 'Deny', answer: { type: 'select', value: 'Deny' }, tone: 'deny' },
  ])
})

it('answers a yes or no gate and each option of another choice', () => {
  expect(approvalChoices({ kind: 'confirm', title: 'Allow bash?', message: 'git push' }).map((choice) => [choice.label, choice.answer])).toEqual([
    ['Allow once', { type: 'confirm', value: true }],
    ['Deny', { type: 'confirm', value: false }],
  ])
  expect(approvalChoices({ kind: 'select', title: 'Pick', options: ['Red', 'Blue'] }).map((choice) => choice.label)).toEqual(['Red', 'Blue'])
  expect(approvalChoices({ kind: 'input', title: 'Name?' })).toEqual([])
  expect(approvalChoices(undefined)).toEqual([])
})

it('names the operation and its inputs', () => {
  expect(approvalSummary({ request: { kind: 'confirm', title: 'Allow bash?', message: 'git push origin main' } })).toEqual({ title: 'Allow bash?', inputs: 'git push origin main' })
  expect(approvalSummary({ request: { kind: 'select', title: 'Allow write?', options: [] } })).toEqual({ title: 'Allow write?', inputs: '' })
  expect(approvalSummary({})).toEqual({ title: 'Approval needed', inputs: '' })
})

it('says when another device answered first or the answer is on its way', () => {
  expect(approvalOutcome('answered')).toBe('')
  expect(approvalOutcome('already_answered')).toBe('Another device answered this first. Its answer stands.')
  expect(approvalOutcome('sent')).toBe('Answer sent. The machine applies it when it answers.')
})
