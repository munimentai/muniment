import { afterEach, describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { expandSidebar, openFirstRunModelSettings } from './e2e/support/first-run.mjs'

afterEach(() => vi.unstubAllGlobals())

describe('The installed app exposes first-run controls.', () => {
  it('The helper opens More providers once it is enabled.', async () => {
    const calls = []
    vi.stubGlobal('$', async (selector) => {
      expect(selector).toBe('button=More providers')
      return {
        waitForDisplayed: async () => calls.push('visible'),
        waitForEnabled: async () => calls.push('enabled'),
        click: async () => calls.push('click'),
      }
    })

    await openFirstRunModelSettings()

    expect(calls).toEqual(['visible', 'enabled', 'click'])
  })

  it.each(['false', 'true'])('The helper keeps the sidebar open when aria-expanded starts at %s.', async (initial) => {
    let expanded = initial
    const click = vi.fn(async () => { expanded = 'true' })
    vi.stubGlobal('$', async (selector) => {
      expect(selector).toBe('button[aria-controls="sidebar"]')
      return { waitForDisplayed: async () => {}, getAttribute: async () => expanded, click }
    })
    vi.stubGlobal('browser', { waitUntil: async (condition) => expect(await condition()).toBe(true) })

    await expandSidebar()

    expect(click).toHaveBeenCalledTimes(initial === 'false' ? 1 : 0)
  })

  it('The helper rejects a sidebar that does not expand.', async () => {
    vi.stubGlobal('$', async () => ({
      waitForDisplayed: async () => {},
      getAttribute: async () => 'false',
      click: async () => {},
    }))
    vi.stubGlobal('browser', { waitUntil: async (condition, { timeoutMsg }) => {
      if (!await condition()) throw new Error(timeoutMsg)
    } })

    await expect(expandSidebar()).rejects.toThrow('The sidebar did not expand.')
  })
})

describe('Candidate source modes share the same specs.', () => {
  const runnerText = (name) => readFileSync(`test/e2e/runner/${name}`, 'utf8')

  it.each([
    ['linux.sh', ['local-mode-chat', 'real-sign-in']],
    ['macos-wdio.sh', ['local-mode-chat', 'real-sign-in']],
    ['windows.ps1', ['local-mode-chat', 'real-sign-in']],
  ])('%s runs its specs outside the source mode branches', (name, specs) => {
    const runner = runnerText(name)
    const sequence = runner.slice(runner.lastIndexOf('# Run the installed chat specs first.') >= 0 ? runner.lastIndexOf('# Run the installed chat specs first.') : runner.indexOf('MUNIMENT_E2E_HOME_PATH'))
    for (const spec of specs) expect(sequence).toContain(`${spec}.spec.js`)
    expect(sequence).not.toMatch(/source_mode|\$sourceMode/)
    expect(sequence).not.toMatch(/skip|--exclude/i)
  })
})
