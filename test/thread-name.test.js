import { expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
// The real harness wire tests cover the bundled import. Unit tests inject completion.
const source = readFileSync('src-tauri/core/src/assistant_identity.mjs', 'utf8').replace(/^import \{ completeSimple \}[^\n]*\n/m, '')
const moduleUrl = `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`
const { nameFirstThread, threadName } = await import(/* @vite-ignore */ moduleUrl)

it('requests a short name separately and saves only the validated result', async () => {
  const editor = vi.fn().mockResolvedValueOnce('{"prompt":"Help me fix the report"}').mockResolvedValue('{}')
  const complete = vi.fn().mockResolvedValue({ content: [{ type: 'text', text: '"Report repair"' }], stopReason: 'stop' })
  await nameFirstThread({ model: { id: 'model' }, ui: { editor }, modelRegistry: { getApiKeyAndHeaders: vi.fn().mockResolvedValue({ ok: true, apiKey: "test-key" }) } }, complete)
  expect(complete.mock.calls[0][1].messages[0].content).toBe('Help me fix the report')
  expect(complete.mock.calls[0][2].maxTokens).toBe(256)
  expect(JSON.parse(editor.mock.calls[1][1])).toEqual({ action: 'save', title: 'Report repair' })
})

it('skips a named thread and lets a failed naming call leave the reply available', async () => {
  const complete = vi.fn().mockRejectedValue(new Error('offline'))
  const context = { model: {}, ui: { editor: vi.fn().mockResolvedValue('{}') }, modelRegistry: { getApiKeyAndHeaders: vi.fn().mockResolvedValue({ ok: true, apiKey: "test-key" }) } }
  await nameFirstThread(context, complete)
  expect(complete).not.toHaveBeenCalled()
  context.ui.editor.mockResolvedValue('{"prompt":"First message"}')
  await expect(nameFirstThread(context, complete)).resolves.toBeUndefined()
  expect(context.ui.editor).toHaveBeenCalledTimes(2)
  for (const name of ['', 'One two three four', 'Two\nlines']) expect(threadName(name)).toBeNull()
})

it('uses the current model registry completion boundary without reading credentials directly', async () => {
  const editor = vi.fn().mockResolvedValueOnce('{"prompt":"Compare leases"}').mockResolvedValue('{}')
  const complete = vi.fn().mockResolvedValue({ content: [{ type: 'text', text: 'Lease comparison' }] })
  await nameFirstThread({ model: {}, ui: { editor }, modelRegistry: { complete } })
  expect(complete).toHaveBeenCalledOnce()
  expect(JSON.parse(editor.mock.calls[1][1]).title).toBe('Lease comparison')
})

it('bounds naming even when the provider ignores cancellation and discards its late result', async () => {
  vi.useFakeTimers()
  try {
    let finish
    const complete = vi.fn(() => new Promise((resolve) => { finish = resolve }))
    const editor = vi.fn().mockResolvedValue('{"prompt":"First request"}')
    const pending = nameFirstThread({ model: {}, ui: { editor }, modelRegistry: { complete } })
    await vi.advanceTimersByTimeAsync(8000)
    await pending
    expect(complete.mock.calls[0][2].signal.aborted).toBe(true)
    finish({ content: [{ type: 'text', text: 'Late title' }] })
    await Promise.resolve()
    expect(editor).toHaveBeenCalledTimes(1)
  } finally { vi.useRealTimers() }
})

it('bounds credential lookup before naming starts', async () => {
  vi.useFakeTimers()
  try {
    const complete = vi.fn()
    const pending = nameFirstThread({ model: {}, ui: { editor: async () => '{"prompt":"First request"}' }, modelRegistry: { getApiKeyAndHeaders: () => new Promise(() => {}) } }, complete)
    await vi.advanceTimersByTimeAsync(8000)
    await pending
    expect(complete).not.toHaveBeenCalled()
  } finally { vi.useRealTimers() }
})
