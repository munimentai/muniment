// @vitest-environment node
import { describe, expect, it } from 'vitest'
import { assess, audit, readPins } from '../scripts/check-agent-dependencies.mjs'

describe('agent dependency freshness and compatibility', () => {
  it('reads all shipped pins instead of a second version list', () => {
    const pins = readPins()
    expect(Object.keys(pins.packages)).toHaveLength(5)
    expect(pins.pi).toMatch(/^\d+\.\d+\.\d+$/)
    expect(pins.claude).toMatch(/^\d+\.\d+\.\d+$/)
  })
  it('flags a newer compatible extension for regression testing', () => {
    expect(assess('extension', '1.0.0', { name: 'extension', version: '1.1.0', peerDependencies: { '@earendil-works/pi-ai': '^0.87.0' } }, '0.87.1').status).toBe('update required')
  })
  it('does not approve latest when its peer range excludes the shipped Pi', () => {
    expect(assess('extension', '2.6.5', { name: 'extension', version: '2.6.5', peerDependencies: { '@earendil-works/pi-coding-agent': '^0.84.0' } }, '0.87.1').status).toBe('peer review required')
  })
  it('rejects invalid or mismatched registry metadata', () => {
    expect(() => assess('extension', '1.0.0', { name: 'other', version: '1.1.0' }, '0.87.1')).toThrow()
    expect(() => assess('extension', '1.0.0', { name: 'extension', version: 'latest' }, '0.87.1')).toThrow()
  })
  it('does not treat a registry outage as a pass', async () => {
    await expect(audit(async () => ({ ok: false, status: 503 }))).rejects.toThrow('503')
  })
  it('checks Claude Code, Pi, and every extension', async () => {
    const { claude, pi, packages } = readPins()
    const pins = { '@anthropic-ai/claude-code': claude, '@earendil-works/pi-coding-agent': pi, ...packages }
    const rows = await audit(async url => {
      const name = decodeURIComponent(new URL(url).pathname.split('/')[1])
      return { ok: true, json: async () => ({ name, version: pins[name] }) }
    })
    expect(rows).toHaveLength(7)
    expect(rows.every(row => row.status === 'current')).toBe(true)
  })
})
