import { describe, expect, it } from 'vitest'
import { emptyRuleForm, ruleFromForm, ruleLabel, sameRule } from './permission-rules.js'

// The rules in core's agent.save fixture.
const command = { request_kind: 'command', resource: { arguments: ['bash', 'git status'], type: 'command', working_directory: '/work/agents/Lead-scout' } }
const tool = { request_kind: 'tool', resource: { input: { id: 'lead-1' }, name: 'mcp__crm__delete_lead', type: 'tool' } }

describe('permission rules', () => {
  it('names each rule by its exact request', () => {
    expect(ruleLabel(command)).toBe('bash git status in /work/agents/Lead-scout')
    expect(ruleLabel(tool)).toBe('mcp__crm__delete_lead {"id":"lead-1"}')
    expect(ruleLabel({ request_kind: 'tool', resource: { type: 'tool', name: 'web_search', input: {} } })).toBe('web_search')
    expect(ruleLabel({ request_kind: 'path', resource: { type: 'path', path: '/work/notes.md', operation: 'write' } })).toBe('write /work/notes.md')
  })

  it('builds the journal policy request from each form', () => {
    expect(ruleFromForm({ ...emptyRuleForm('command'), command: ' git status ', directory: '/work/agents/Lead-scout' })).toEqual({
      request_kind: 'command', resource: { type: 'command', arguments: ['bash', 'git status'], working_directory: '/work/agents/Lead-scout' },
    })
    expect(sameRule(ruleFromForm({ ...emptyRuleForm('tool'), name: 'mcp__crm__delete_lead', input: '{"id":"lead-1"}' }), {
      request_kind: 'tool', resource: { type: 'tool', name: 'mcp__crm__delete_lead', input: { id: 'lead-1' } },
    })).toBe(true)
    expect(ruleFromForm({ ...emptyRuleForm('path'), path: '/work/notes.md', operation: 'write' })).toEqual({
      request_kind: 'path', resource: { type: 'path', path: '/work/notes.md', operation: 'write' },
    })
  })

  it('names the missing field', () => {
    expect(() => ruleFromForm(emptyRuleForm('command'))).toThrow('Enter the command and the folder it runs in.')
    expect(() => ruleFromForm(emptyRuleForm('path'))).toThrow('Enter the full path of the file.')
    expect(() => ruleFromForm(emptyRuleForm('tool'))).toThrow('Enter the tool name.')
    expect(() => ruleFromForm({ ...emptyRuleForm('tool'), name: 'x', input: '{' })).toThrow('Enter the tool input as JSON.')
  })
})
