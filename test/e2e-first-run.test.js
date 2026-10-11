import { afterEach, describe, expect, it, vi } from 'vitest'
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
