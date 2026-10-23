import { describe, expect, it, vi } from 'vitest'
import { createFolderAccessWatch, folderAccessHeading } from './folder-access.js'

const waiting = { state: 'waiting', folder: 'Documents', message: 'macOS is asking whether Muniment may open your Documents folder. Choose Allow in that prompt, then try again.' }
const denied = { state: 'denied', folder: 'Documents', message: 'macOS does not let Muniment open your Documents folder. Turn on Muniment in System Settings > Privacy & Security > Files & Folders, then try again.' }
const open = { state: 'open', folder: 'Documents', message: null }

function answers(...replies) {
  const calls = []
  const invoke = vi.fn(async (command, payload) => {
    calls.push([command, payload])
    if (command !== 'home_folder_state') return undefined
    const reply = replies.shift()
    return typeof reply === 'function' ? reply() : reply
  })
  return { invoke, calls }
}

describe('folder access watch', () => {
  it('names a pending prompt, waits for its answer and clears once Home opens', async () => {
    const { invoke, calls } = answers(waiting, open)
    const states = []
    const watch = createFolderAccessWatch({ invoke, homePath: '/Users/a/Documents/muniment', onChange: (next) => states.push(next), wait: async () => {} })
    await watch.start()
    expect(states).toEqual([{ state: 'waiting', folder: 'Documents', message: waiting.message }, null])
    expect(calls).toEqual([
      ['home_folder_state', { homePath: '/Users/a/Documents/muniment', wait: false }],
      ['home_folder_state', { homePath: '/Users/a/Documents/muniment', wait: true }],
    ])
  })

  it('reads a refusal again after a pause until macOS lets Muniment in', async () => {
    const { invoke } = answers(denied, denied, open)
    const wait = vi.fn(async () => {})
    const states = []
    const watch = createFolderAccessWatch({ invoke, homePath: '/h', onChange: (next) => states.push(next?.state ?? null), wait, deniedPoll: 5000 })
    await watch.start()
    expect(states).toEqual(['denied', 'denied', null])
    expect(wait).toHaveBeenCalledTimes(2)
    expect(wait).toHaveBeenCalledWith(5000)
  })

  it('stays silent for an open Home and for a failed read', async () => {
    for (const reply of [open, () => { throw new Error('no bridge') }]) {
      const { invoke } = answers(reply)
      const onChange = vi.fn()
      await createFolderAccessWatch({ invoke, homePath: '/h', onChange }).start()
      expect(onChange).toHaveBeenCalledWith(null)
      expect(onChange).not.toHaveBeenCalledWith(expect.objectContaining({ state: expect.any(String) }))
    }
  })

  it('opens Files & Folders for a refusal and reads Home again for a pending prompt', async () => {
    let release
    const held = new Promise((resolve) => { release = resolve })
    const { invoke, calls } = answers(denied, () => held)
    const states = []
    const watch = createFolderAccessWatch({ invoke, homePath: '/h', onChange: (next) => states.push(next?.state ?? null), wait: () => held })
    void watch.start()
    await vi.waitFor(() => expect(states).toEqual(['denied']))
    await watch.allow()
    expect(calls.map(([command]) => command)).toContain('home_folder_settings')
    watch.stop()
    release(open)

    const pending = answers(waiting, () => new Promise(() => {}), open)
    const pendingStates = []
    const pendingWatch = createFolderAccessWatch({ invoke: pending.invoke, homePath: '/h', onChange: (next) => pendingStates.push(next?.state ?? null) })
    void pendingWatch.start()
    await vi.waitFor(() => expect(pendingStates).toEqual(['waiting']))
    await pendingWatch.allow()
    expect(pending.calls.map(([command]) => command)).not.toContain('home_folder_settings')
    expect(pending.calls.at(-1)).toEqual(['home_folder_state', { homePath: '/h', wait: false }])
    expect(pendingStates).toEqual(['waiting', null])
  })

  it('never reads without a Home and stops publishing once stopped', async () => {
    const { invoke } = answers(waiting)
    const onChange = vi.fn()
    await createFolderAccessWatch({ invoke, homePath: '', onChange }).start()
    expect(invoke).not.toHaveBeenCalled()
    const watch = createFolderAccessWatch({ invoke, homePath: '/h', onChange })
    watch.stop()
    await watch.start()
    expect(onChange).not.toHaveBeenCalled()
  })

  it('heads the card with the folder macOS guards', () => {
    expect(folderAccessHeading(waiting)).toBe('Waiting for Documents access')
    expect(folderAccessHeading(denied)).toBe('No access to Documents')
    expect(folderAccessHeading({ state: 'waiting', folder: null })).toBe('Waiting for Home access')
  })
})
