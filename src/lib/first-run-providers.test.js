import { describe, expect, it } from 'vitest'
import { SUBSCRIPTIONS, connectOptions } from './first-run-providers.js'

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
})
