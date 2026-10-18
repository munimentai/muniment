import { describe, expect, it } from 'vitest'
import { BUDGETS, compare, parseArgs, parseFootprint, parsePs, percentile } from '../scripts/speed-timing.mjs'

describe('speed timing script', () => {
  it('takes one target and the documented options', () => {
    expect(parseArgs(['--probe'])).toMatchObject({ probe: true, out: 'speed-report.json', idle: 60, port: 4445, strict: false })
    expect(parseArgs(['--app', '/Applications/muniment.app/Contents/MacOS/muniment', '--idle', '5', '--strict', '--home', '/tmp/home'])).toMatchObject({ app: '/Applications/muniment.app/Contents/MacOS/muniment', idle: 5, strict: true, home: '/tmp/home' })
    expect(() => parseArgs([])).toThrow('Pass one of')
    expect(() => parseArgs(['--probe', '--webdriver', 'http://127.0.0.1:4445'])).toThrow('Pass one of')
    expect(() => parseArgs(['--probe', '--fast'])).toThrow('Unknown option: --fast')
  })

  it('reads process samples and the macOS footprint', () => {
    expect(parsePs(' 101   0:01.50  20480\n 202 1-02:03:04  1024\n')).toEqual([
      { pid: 101, cpuSeconds: 1.5, rssKiB: 20480 },
      { pid: 202, cpuSeconds: 86_400 + 7_384, rssKiB: 1024 },
    ])
    expect(parseFootprint('muniment [101]: 64-bit  Footprint: 182 MB (16384 bytes per page)')).toBe(182)
    expect(parseFootprint('Footprint: 1.5 GB')).toBe(1536)
    expect(parseFootprint('no summary')).toBeNull()
  })

  it('compares each metric to its budget and to the last report', () => {
    expect(percentile([5, 1, 9, 3], 0.5)).toBe(3)
    expect(percentile([5, 1, 9, 3], 0.95)).toBe(9)
    expect(percentile([], 0.5)).toBeNull()
    const results = compare({ keystrokeP95Ms: 4, idleCpuPercent: 0.9, launchToComposerMs: 800 }, BUDGETS, { metrics: { launchToComposerMs: 600 } })
    const byMetric = Object.fromEntries(results.map((result) => [result.metric, result]))
    expect(byMetric.keystrokeP95Ms.pass).toBe(true)
    expect(byMetric.idleCpuPercent.pass).toBe(false)
    // Within budget, but more than a tenth over the last nightly.
    expect(byMetric.launchToComposerMs).toMatchObject({ value: 800, baseline: 600, pass: false })
    expect(byMetric.sendToFirstTokenMs.pass).toBeNull()
  })
})
