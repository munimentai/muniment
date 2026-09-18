import { describe, expect, it } from 'vitest'

import { reportErrorLine, reportGroups, reportKindLine, reportLine, reportTrend } from './record-report.js'

describe('the local report', () => {
  const report = {
    open: 2,
    moved: -1,
    kinds: [{ name: 'person', count: 4 }, { name: 'org', count: 2 }, { name: 'deal', count: 0 }],
    findings: [
      { id: 'duplicate-person', category: 'identity', magnitude: 2, claim: 'people appear more than once', consequence: 'reports read them twice', evidence: ['person 4 rows'] },
      { id: 'stale-value', category: 'freshness', magnitude: 1, claim: 'records hold an old value', consequence: 'the older copy answers first', evidence: [] },
    ],
  }

  it('reads the header as the company, the open count and the trend', () => {
    expect(reportLine('Northwind', report)).toBe('Northwind · 2 open findings · 1 fewer than the last read')
    expect(reportLine('Northwind', { open: 1, moved: null })).toBe('Northwind · 1 open finding · first read')
    expect(reportLine(null, { open: 0, moved: 0 })).toBe('This company · 0 open findings · level with the last read')
    expect(reportTrend(3)).toBe('3 more than the last read')
  })

  it('reads the kind holdings largest first and skips empty kinds', () => {
    expect(reportKindLine(report.kinds)).toBe('person 4 · org 2')
    expect(reportKindLine(null)).toBe('')
  })

  it('groups the findings by category with the largest group first', () => {
    const groups = reportGroups(report)
    expect(groups.map((group) => [group.label, group.count])).toEqual([['Identity', 1], ['Freshness', 1]])
    expect(reportGroups(null)).toEqual([])
  })

  it('reads a failure as one sentence', () => {
    expect(reportErrorLine({ error: { message: 'The record is locked. Try again.' } })).toBe('The record is locked.')
    expect(reportErrorLine(null)).toBe('The report did not answer.')
  })
})
