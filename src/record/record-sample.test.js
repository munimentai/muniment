import { describe, expect, it } from 'vitest'

import {
  SAMPLE_COMPANY,
  findingClaim,
  findingSentence,
  groupFindings,
  sampleFindings,
  sampleKindLine,
  sampleKinds,
  sampleProofLine,
  sampleLeadLine,
  sampleReportLine,
  sampleTrend,
  topFindings,
} from './record-sample.js'

describe('the sample company', () => {
  it('hands out copies, so a caller cannot edit the fixture', () => {
    const findings = sampleFindings()
    findings[0].magnitude = 1
    findings[0].evidence.push('drift')
    expect(sampleFindings()[0].magnitude).not.toBe(1)
    expect(sampleFindings()[0].evidence).not.toContain('drift')
    const kinds = sampleKinds()
    kinds[0].count = 1
    expect(sampleKinds()[0].count).not.toBe(1)
  })

  it('carries a claim, a magnitude and a consequence on every finding', () => {
    for (const finding of sampleFindings()) {
      expect(finding.magnitude).toBeGreaterThan(0)
      expect(finding.claim).toBeTruthy()
      expect(finding.consequence).toBeTruthy()
      expect(finding.evidence.length).toBeGreaterThan(0)
    }
  })

  it('reads one finding as one sentence', () => {
    expect(findingSentence(sampleFindings()[0]))
      .toBe('4,182 people appear more than once across HubSpot and Salesforce, so every owner report and every campaign count reads them twice.')
  })
})

describe('topFindings and findingClaim', () => {
  it('takes the largest findings, largest first', () => {
    expect(topFindings(3).map((finding) => finding.magnitude)).toEqual([4182, 2914, 1207])
    expect(topFindings(1)[0].id).toBe('duplicate-person')
    expect(topFindings(99)).toHaveLength(sampleFindings().length)
  })

  it('reads a claim without its magnitude, so a row can set the number beside it', () => {
    const finding = topFindings(1)[0]
    expect(findingClaim(finding)).toBe('people appear more than once across HubSpot and Salesforce, so every owner report and every campaign count reads them twice.')
    expect(findingSentence(finding)).toBe(`4,182 ${findingClaim(finding)}`)
  })
})

describe('sampleLeadLine', () => {
  it('names the company as the sample it is', () => {
    expect(sampleLeadLine()).toBe('Northwind Traders, a sample company · 8 open findings · 3 fewer than the last run')
  })
})

describe('sampleProofLine', () => {
  it('names the three largest findings, largest first', () => {
    expect(sampleProofLine()).toBe('4,182 duplicate people · 2,914 stale values · 1,207 duplicate organizations')
  })

  it('reads the findings it is given', () => {
    const findings = [
      { magnitude: 2, short: 'two' },
      { magnitude: 9, short: 'nine' },
      { magnitude: 5, short: 'five' },
      { magnitude: 1, short: 'one' },
    ]
    expect(sampleProofLine(findings)).toBe('9 nine · 5 five · 2 two')
  })
})

describe('sampleReportLine', () => {
  it('names the company, the open count and the trend', () => {
    expect(sampleReportLine()).toBe('Northwind Traders · 8 open findings · 3 fewer than the last run')
    expect(SAMPLE_COMPANY).toBe('Northwind Traders')
  })

  it('counts one finding in the singular', () => {
    expect(sampleReportLine([{ magnitude: 1 }], 2)).toBe('Northwind Traders · 1 open finding · 2 more than the last run')
  })
})

describe('sampleTrend', () => {
  it('reads a rise, a fall and no move', () => {
    expect(sampleTrend(4)).toBe('4 more than the last run')
    expect(sampleTrend(-4)).toBe('4 fewer than the last run')
    expect(sampleTrend(0)).toBe('level with the last run')
  })
})

describe('groupFindings', () => {
  it('orders groups and rows by magnitude, descending', () => {
    const groups = groupFindings()
    expect(groups.map((group) => group.label)).toEqual(['Identity', 'Freshness', 'Ownership', 'Schema'])
    expect(groups.map((group) => group.count)).toEqual([3, 2, 1, 2])
    for (const group of groups) {
      const magnitudes = group.findings.map((finding) => finding.magnitude)
      expect(magnitudes).toEqual([...magnitudes].sort((a, b) => b - a))
    }
  })

  it('holds every finding exactly once', () => {
    const grouped = groupFindings().flatMap((group) => group.findings.map((finding) => finding.id))
    expect(grouped.sort()).toEqual(sampleFindings().map((finding) => finding.id).sort())
  })
})

describe('sampleKindLine', () => {
  it('reads one mono line of what the sample holds', () => {
    expect(sampleKindLine()).toBe('person 18,340 · organization 6,210 · invoice 12,004 · ticket 9,088 · deal 1,455 · subscription 712')
  })
})
