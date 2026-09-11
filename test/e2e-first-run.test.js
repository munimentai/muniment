import { afterEach, describe, expect, it, vi } from 'vitest'
import { expandSidebar, openFirstRunModelSettings } from './e2e/support/first-run.mjs'

afterEach(() => vi.unstubAllGlobals())

describe('The installed app exposes first-run controls.', () => {
  it('The helper checks the model panel after Send and waits for model settings.', async () => {
    const calls = []
    const model = {
      waitForDisplayed: async () => calls.push('model visible'),
      getAttribute: async () => 'true',
    }
    const settings = {
      waitForDisplayed: async () => calls.push('settings visible'),
      waitForEnabled: async () => calls.push('settings ready'),
      click: async () => calls.push('open settings'),
    }
    const panel = {
      waitForDisplayed: async () => calls.push('panel visible'),
      $: async (selector) => {
        if (selector === 'p=No free hosted model exists at the no-account tier.') return { isDisplayed: async () => true }
        if (selector === 'button=Open model settings') return settings
        throw new Error(`The panel selector ${selector} is unexpected.`)
      },
    }
    vi.stubGlobal('expect', expect)
    vi.stubGlobal('$', async (selector) => {
      if (selector === '[data-testid="onboarding-model"]') return model
      if (selector === '#onboarding-model-panel') return panel
      if (selector === 'button=Send') return { click: async () => calls.push('send') }
      throw new Error(`The shell selector ${selector} is unexpected.`)
    })

    await openFirstRunModelSettings()

    expect(calls).toEqual(['send', 'model visible', 'panel visible', 'settings visible', 'settings ready', 'open settings'])
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
