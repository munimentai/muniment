import { afterEach, describe, expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { expandSidebar, firstRunState, openFirstRunModelSettings, openSettings, openSettingsPage } from './e2e/support/first-run.mjs'
import { replySnapshot, waitForReply } from './e2e/support/chat-reply.mjs'

afterEach(() => vi.unstubAllGlobals())

describe('The installed app exposes first-run controls.', () => {
  it('The helper opens More providers once it is enabled.', async () => {
    const calls = []
    const catalog = { isDisplayed: async () => true }
    vi.stubGlobal('$', async (selector) => {
      if (selector === '[role="dialog"][aria-label="Settings"]') return { isDisplayed: async () => true, $: async () => catalog }
      if (selector === 'section.onboarding p.error[role="alert"]') return { isDisplayed: async () => false }
      expect(selector).toBe('button=More providers')
      return {
        waitForDisplayed: async () => calls.push('visible'),
        waitForEnabled: async () => calls.push('enabled'),
        click: async () => calls.push('click'),
      }
    })
    vi.stubGlobal('browser', { waitUntil: async (condition) => expect(await condition()).toBe(true) })

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

// A page fake for the first-run transition: More providers, the first-run
// alert and the Settings dialog with its catalog heading, each read per poll.
function firstRunPage(state) {
  const clicks = []
  vi.stubGlobal('$', async (selector) => {
    if (selector === 'button=More providers') {
      return { waitForDisplayed: async () => {}, waitForEnabled: async () => {}, click: async () => clicks.push(selector) }
    }
    if (selector === 'section.onboarding p.error[role="alert"]') {
      return { isDisplayed: async () => !!state.error, getText: async () => state.error ?? '' }
    }
    if (selector === '[role="dialog"][aria-label="Settings"]') {
      return {
        isDisplayed: async () => state.polls >= state.dialogAfter,
        $: async (child) => {
          expect(child).toBe('h4=Connect account')
          return { isDisplayed: async () => state.polls >= state.catalogAfter }
        },
      }
    }
    throw new Error(`unexpected selector ${selector}`)
  })
  vi.stubGlobal('browser', {
    execute: vi.fn(async () => ({ home: { configured: true, path: true }, settingsDialog: false, firstRun: true })),
    waitUntil: async (condition, { timeoutMsg }) => {
      for (state.polls = 0; state.polls < 5; state.polls += 1) if (await condition()) return true
      throw new Error(timeoutMsg)
    },
  })
  return clicks
}

describe('The first-run helper observes the real transition.', () => {
  it('The helper waits for a delayed dialog and catalog after one click.', async () => {
    const clicks = firstRunPage({ dialogAfter: 2, catalogAfter: 3 })
    await expect(openFirstRunModelSettings()).resolves.toBeTruthy()
    expect(clicks).toEqual(['button=More providers'])
  })

  it('The helper names the first-run error and the startup state of a failed transition.', async () => {
    const clicks = firstRunPage({ dialogAfter: Infinity, catalogAfter: Infinity, error: 'The runtime is not connected yet. Choose a model again.' })
    await expect(openFirstRunModelSettings()).rejects.toThrow(/More providers failed: The runtime is not connected yet\. Choose a model again\. First-run state: .*"firstRun":true/)
    expect(clicks).toEqual(['button=More providers'])
  })

  it('The helper rejects a dialog that never shows the catalog, with one click.', async () => {
    const clicks = firstRunPage({ dialogAfter: 0, catalogAfter: Infinity })
    await expect(openFirstRunModelSettings()).rejects.toThrow(/More providers did not open the provider catalog in Settings\. First-run state:/)
    expect(clicks).toEqual(['button=More providers'])
  })

  it('The Settings helpers use the sidebar gear and the Settings sections.', async () => {
    let open = false
    const clicked = []
    vi.stubGlobal('$', async (selector) => {
      if (selector === 'button[aria-controls="sidebar"]') return { waitForDisplayed: async () => {}, getAttribute: async () => 'true' }
      if (selector === '[role="dialog"][aria-label="Settings"]') return { isDisplayed: async () => open, waitForDisplayed: async () => { if (!open) throw new Error('closed') } }
      if (selector === 'button.dock-button[aria-label="Settings"]') return { waitForDisplayed: async () => {}, click: async () => { clicked.push('gear'); open = true } }
      if (selector === 'nav[aria-label="Settings sections"]') {
        return { waitForDisplayed: async () => {}, $: async (button) => ({ click: async () => clicked.push(button) }) }
      }
      throw new Error(`unexpected selector ${selector}`)
    })
    vi.stubGlobal('browser', { waitUntil: async (condition) => expect(await condition()).toBe(true) })

    await openSettingsPage('Data')
    await openSettings()

    expect(clicked).toEqual(['gear', 'button=Data'])
  })
})

describe('The first-run state reads the commands as the app answers them.', () => {
  it('reports a Home path from the camelCase home_status answer without the path itself', async () => {
    const answers = {
      home_status: { configured: true, homePath: 'C:\\Users\\runner\\Documents\\Muniment' },
      local_mode_status: true,
      attach_listener_status: { connected: true, supervisor_running: true },
    }
    vi.stubGlobal('window', { ...window, __TAURI__: { core: { invoke: async (command) => answers[command] } } })
    vi.stubGlobal('browser', { execute: async (fn) => fn() })
    const state = await firstRunState()
    expect(state).toContain('"home":{"configured":true,"path":true}')
    expect(state).not.toContain('Users')
  })
})

describe('The local-mode spec reads the reply of its own prompt.', () => {
  const page = (html) => {
    document.body.innerHTML = html
    return document.querySelector('.response')
  }

  it('reads streaming text without the caret', () => {
    const response = page('<div class="response"><div class="streaming"><div class="assistant-markdown"><p>Sure, start<span class="caret">_</span></p></div></div></div>')
    expect(replySnapshot(response)).toEqual({ text: 'Sure, start', error: '', streaming: true })
  })

  it('reads completed text with a plain receipt', () => {
    const response = page('<div class="response"><div class="assistant-markdown"><p>Done.</p></div><div class="receipt-line"><p class="provenance">Ollama</p></div></div>')
    expect(replySnapshot(response)).toEqual({ text: 'Done.', error: '', streaming: false })
  })

  it('does not count an empty streaming response or its caret', () => {
    const response = page('<div class="response"><div class="streaming"><div class="assistant-markdown"><span class="caret">_</span></div></div><button class="provenance">x</button></div>')
    expect(replySnapshot(response).text).toBe('')
  })

  it('does not count user text or a missing response', () => {
    page('<div class="user-turn"><p>Prompt</p></div><div class="response"></div>')
    expect(replySnapshot(document.querySelector('.response')).text).toBe('')
    expect(replySnapshot(null).text).toBe('')
  })

  it('reads an error message as a failure, not a reply', () => {
    const response = page('<div class="response"><div class="run-error">The model refused. <button>Try again</button></div></div>')
    expect(replySnapshot(response)).toEqual({ text: '', error: 'The model refused. Try again', streaming: false })
  })

  const waitFor = (snapshots) => {
    const response = { isExisting: async () => true }
    let index = 0
    vi.stubGlobal('browser', {
      execute: vi.fn(async (fn, element) => {
        expect(fn).toBe(replySnapshot)
        expect(element).toBe(response)
        return snapshots[Math.min(index++, snapshots.length - 1)]
      }),
      waitUntil: async (condition, { timeoutMsg }) => {
        for (let poll = 0; poll < snapshots.length; poll += 1) if (await condition()) return true
        throw new Error(timeoutMsg)
      },
    })
    return response
  }

  it('waits through an empty response for streaming text', async () => {
    const response = waitFor([{ text: '', error: '' }, { text: 'Hello', error: '', streaming: true }])
    await expect(waitForReply(response, 'p')).resolves.toMatchObject({ text: 'Hello', streaming: true })
  })

  it('accepts completed text', async () => {
    const response = waitFor([{ text: 'Done.', error: '', streaming: false }])
    await expect(waitForReply(response, 'p')).resolves.toMatchObject({ text: 'Done.' })
  })

  it('rejects a response that stays empty', async () => {
    const response = waitFor([{ text: '', error: '' }, { text: '', error: '' }])
    await expect(waitForReply(response, 'prompt 1')).rejects.toThrow('local-mode chat did not render its first reply for prompt: prompt 1')
  })

  it('rejects a refusal with its message', async () => {
    const response = waitFor([{ text: '', error: 'chat_not_entitled: No model.' }])
    await expect(waitForReply(response, 'p')).rejects.toThrow('The local-mode chat response failed: chat_not_entitled: No model.')
  })
})

describe('The local-mode spec sends a plain prompt.', () => {
  it('does not count tool activity or a question card as a reply', () => {
    document.body.innerHTML = '<div class="response"><div class="tool-card"><p>ls</p></div><div class="question-card"><p>Which folder?</p></div></div>'
    expect(replySnapshot(document.querySelector('.response')).text).toBe('')
  })

  it('names its prompt with a unique marker and adds no instruction', () => {
    const spec = readFileSync('test/e2e/specs/local-mode-chat.spec.js', 'utf8')
    const prompt = spec.match(/const prompt = `([^`]*)`/)[1]
    expect(prompt).toBe('Muniment local E2E chat ${Date.now()}')
    expect(spec).not.toMatch(/Do not use any tools|plain-text sentence/)
    expect(spec).toContain('The local-mode chat selected')
    expect(spec).toContain('waitForReply(response, prompt)')
    expect(spec).toContain('timeout: 180000')
  })
})
