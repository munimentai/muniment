import { describe, expect, it } from 'vitest'
import { SUBSCRIPTIONS, connectOptions, serverDetail } from './first-run-providers.js'

const server = (id, state, extra = {}) => ({ id, name: id, baseUrl: `http://127.0.0.1:1/v1`, state, modelCount: 2, ...extra })

describe('first-run connect options', () => {
  it('lists every subscription in catalog order when the scan finds nothing', () => {
    const options = connectOptions({ servers: [], hints: [] })
    expect(options.map((option) => option.id)).toEqual(SUBSCRIPTIONS.map((entry) => entry.id))
    expect(options.every((option) => option.kind === 'subscription' && !option.hint)).toBe(true)
    expect(connectOptions(null).map((option) => option.id)).toEqual(SUBSCRIPTIONS.map((entry) => entry.id))
  })

  it('sorts ready servers, hinted subscriptions, servers that need a step, then the rest', () => {
    const options = connectOptions({
      servers: [server('lmstudio', 'not-running'), server('omlx', 'needs-key'), server('ollama', 'ready')],
      hints: [{ provider: 'openai', evidence: 'Codex CLI is signed in' }, { provider: 'anthropic', evidence: 'Claude Code is signed in' }],
    })
    expect(options.slice(0, 5).map((option) => option.id)).toEqual(['ollama', 'anthropic', 'openai', 'omlx', 'lmstudio'])
    expect(options.find((option) => option.id === 'openai').hint.evidence).toBe('Codex CLI is signed in')
    expect(options.slice(5).map((option) => option.id)).toEqual(SUBSCRIPTIONS.map((entry) => entry.id).filter((id) => !['anthropic', 'openai'].includes(id)))
  })

  it('describes a server by its loopback address and model count', () => {
    expect(serverDetail(server('omlx', 'needs-key', { baseUrl: 'http://127.0.0.1:8080/v1', modelCount: 8 }))).toBe('127.0.0.1:8080 · 8 models')
    expect(serverDetail(server('ollama', 'ready', { baseUrl: 'http://127.0.0.1:11434/v1', modelCount: 1 }))).toBe('127.0.0.1:11434 · 1 model')
    expect(serverDetail(server('lmstudio', 'not-running'))).toBe('Installed')
  })
})
