import { afterEach, expect, it, vi } from 'vitest'
import routingProgress from '../src-tauri/core/src/routing_progress.mjs'

const contexts = []
afterEach(async () => {
  for (const hooks of contexts.splice(0)) await hooks.agent_end()
  vi.useRealTimers()
  vi.unstubAllGlobals()
})
function setup(baseUrl = 'http://127.0.0.1:1234/v1', provider = 'muniment-router') {
  vi.useFakeTimers()
  const hooks = {}
  routingProgress({ on: (name, handler) => { hooks[name] = handler } })
  contexts.push(hooks)
  const ctx = { model: { provider, baseUrl }, ui: { notify: vi.fn() } }
  const event = { headers: { Authorization: 'Bearer test-token' } }
  return { hooks, ctx, event }
}
it('forwards ordered router stages once and drains before response output', async () => {
  const { hooks, ctx, event } = setup()
  const fetch = vi.fn().mockResolvedValueOnce({ ok: false }).mockResolvedValueOnce({ ok: true, json: async () => ({ stages: ['choosing-model', 'waiting-for-account'] }) }).mockResolvedValue({ ok: true, json: async () => ({ stages: ['choosing-model', 'waiting-for-account', 'fallback', 'waiting-for-account', 'thinking'], done: true }) })
  vi.stubGlobal('fetch', fetch)
  await hooks.before_provider_headers(event, ctx)
  expect(event.headers['x-muniment-routing-id']).toMatch(/^[a-f\d-]{36}$/)
  await vi.advanceTimersByTimeAsync(300)
  expect(ctx.ui.notify.mock.calls.map(([text]) => text)).toEqual(['muniment:routing:choosing-model', 'muniment:routing:waiting-for-account'])
  await hooks.after_provider_response()
  expect(ctx.ui.notify.mock.calls.map(([text]) => text)).toEqual(['muniment:routing:choosing-model', 'muniment:routing:waiting-for-account', 'muniment:routing:fallback', 'muniment:routing:waiting-for-account', 'muniment:routing:thinking'])
  expect(fetch.mock.calls[0][1].headers.authorization).toBe('Bearer test-token')
  const calls = fetch.mock.calls.length
  await vi.advanceTimersByTimeAsync(2000)
  expect(fetch).toHaveBeenCalledTimes(calls)
})
it('does not leak router credentials to another provider or a remote URL', async () => {
  const fetch = vi.fn()
  vi.stubGlobal('fetch', fetch)
  for (const [baseUrl, provider] of [['https://example.com/v1', 'muniment-router'], ['http://127.0.0.1:1234', 'openai']]) {
    const { hooks, ctx, event } = setup(baseUrl, provider)
    await hooks.before_provider_headers(event, ctx)
    expect(event.headers['x-muniment-routing-id']).toBeUndefined()
  }
  await vi.advanceTimersByTimeAsync(500)
  expect(fetch).not.toHaveBeenCalled()
})
it('drops a late response after cancellation and uses a new ID for the next request', async () => {
  const { hooks, ctx, event } = setup()
  let resolve
  vi.stubGlobal('fetch', vi.fn().mockImplementation(() => new Promise(r => { resolve = r })))
  await hooks.before_provider_headers(event, ctx)
  const first = event.headers['x-muniment-routing-id']
  await vi.advanceTimersByTimeAsync(150)
  await hooks.agent_end()
  resolve({ ok: true, json: async () => ({ stages: ['fallback'] }) })
  await vi.advanceTimersByTimeAsync(0)
  expect(ctx.ui.notify).not.toHaveBeenCalled()
  await hooks.before_provider_headers(event, ctx)
  expect(event.headers['x-muniment-routing-id']).not.toBe(first)
})

it('uses the model registry when the SDK adds authorization after the hook', async () => {
  const { hooks, ctx, event } = setup()
  event.headers = {}
  ctx.modelRegistry = { getApiKeyAndHeaders: vi.fn().mockResolvedValue({ ok: true, apiKey: 'registry-key' }) }
  const fetch = vi.fn().mockResolvedValue({ ok: true, json: async () => ({ stages: ['waiting-for-account'] }) })
  vi.stubGlobal('fetch', fetch)
  await hooks.before_provider_headers(event, ctx)
  await vi.advanceTimersByTimeAsync(150)
  expect(fetch.mock.calls[0][1].headers.authorization).toBe('Bearer registry-key')
  expect(ctx.ui.notify).toHaveBeenCalledWith('muniment:routing:waiting-for-account', 'info')
})
