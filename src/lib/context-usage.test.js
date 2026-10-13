import { expect, it } from 'vitest'
import { compactCommand, compactionCounts, compactsOnSwitch, contextHoverLabel, contextLevel, contextMeterShown, contextPercentLabel, windowLabel, windowTokens } from './context-usage.js'

const usage = (fields = {}) => ({ tokens: 46210, contextWindow: 200000, percent: 23.1, estimated: false, windowKnown: true, provider: 'anthropic', model: 'claude-sonnet', autoCompact: true, compactAt: 183616, compacting: false, ...fields })

it('levels the meter by the compaction point and the known window', () => {
  expect(contextLevel(usage())).toBe('normal')
  expect(contextLevel(usage({ tokens: 160000, percent: 80 }))).toBe('caution')
  expect(contextLevel(usage({ windowKnown: false }))).toBe('unknown')
  expect(contextLevel(usage({ compactAt: null, tokens: 171000, percent: 85.5 }))).toBe('caution')
})

it('labels the percent, the estimate and an unknown window', () => {
  expect(contextPercentLabel(usage())).toBe('23%')
  expect(contextPercentLabel(usage({ estimated: true, tokens: 31900, percent: 15.95 }))).toBe('~16%')
  expect(contextPercentLabel(usage({ windowKnown: false }))).toBe('?')
  expect(contextHoverLabel(usage())).toBe('46,210 of 200,000 tokens')
  expect(contextHoverLabel(usage({ estimated: true, tokens: 31900 }))).toBe('About 31,900 tokens')
  expect(contextHoverLabel(usage({ tokens: null, percent: null }))).toBe('Context usage appears after the next reply')
})

it('shows the meter from a quarter of the window, and always for an unknown window', () => {
  expect(contextMeterShown(usage({ tokens: 40000, percent: 20 }))).toBe(false)
  expect(contextMeterShown(usage({ tokens: 50000, percent: 25 }))).toBe(true)
  expect(contextMeterShown(usage({ estimated: true, tokens: 31900, percent: 15.95 }))).toBe(false)
  expect(contextMeterShown(usage({ windowKnown: false, tokens: 2000, percent: 1 }))).toBe(true)
  expect(contextMeterShown(null)).toBe(false)
})

it('reads and labels windows and tells when a switch compacts', () => {
  expect(windowTokens('128K')).toBe(128000)
  expect(windowTokens('1.05M')).toBe(1050000)
  expect(windowTokens('')).toBeNull()
  expect(windowLabel(200000)).toBe('200K')
  expect(windowLabel(1000000)).toBe('1M')
  expect(windowLabel(1050000)).toBe('1.05M')
  expect(compactsOnSwitch(usage(), 40000)).toBe(true)
  expect(compactsOnSwitch(usage(), 128000)).toBe(false)
  expect(compactsOnSwitch(usage({ tokens: null }), 40000)).toBe(false)
})

it('reads /compact with an optional focus and nothing else', () => {
  expect(compactCommand('/compact')).toEqual({ instructions: null })
  expect(compactCommand('  /compact keep the schema decisions ')).toEqual({ instructions: 'keep the schema decisions' })
  expect(compactCommand('/compacted')).toBeNull()
  expect(compactCommand('please /compact')).toBeNull()
})

it('reads the counts a compaction records', () => {
  const detail = 'Context compacted. Earlier messages remain in the conversation.\nContext before compaction: 152400 tokens.\nContext after compaction: about 31900 tokens.\nGoal: retries'
  expect(compactionCounts(detail)).toBe('152,400 → about 31,900 tokens')
  expect(compactionCounts('Context before compaction: 152400 tokens.\nSummary')).toBe('152,400 tokens before')
  expect(compactionCounts('Context compacted.')).toBe('')
})
