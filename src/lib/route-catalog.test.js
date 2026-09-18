import { describe, expect, it } from 'vitest'
import { ROUTE_MODELS, connectedFamilies, familyModels, findModel, routeModel, routeReady, suggestRoutes, suggestedFallback } from './route-catalog.js'

const accounts = [
  { family: 'openai', enabled: true },
  { family: 'openai', enabled: true },
  { family: 'anthropic', enabled: true },
  { family: 'xai', enabled: false },
]

describe('route catalog', () => {
  it('carries one route model per family and reads a model back by name', () => {
    for (const family of Object.keys(ROUTE_MODELS)) {
      expect(routeModel(family)).toBeTruthy()
      expect(familyModels(family).length).toBeGreaterThan(0)
    }
    expect(routeModel('openai').model).toBe('gpt-6-astra')
    expect(routeModel('xai').model).toBe('grok-4.6')
    expect(findModel('anthropic', 'claude-haiku-4-5').tier).toBe('fast')
    expect(findModel('anthropic', 'nothing')).toBe(null)
    expect(familyModels('nobody')).toEqual([])
    expect(routeModel('nobody')).toBe(null)
  })

  it('counts a family as connected on one enabled account', () => {
    expect(connectedFamilies(accounts)).toEqual(['openai', 'anthropic'])
    expect(connectedFamilies([])).toEqual([])
    expect(routeReady({ family: 'openai' }, accounts)).toBe(true)
    expect(routeReady({ family: 'xai' }, accounts)).toBe(false)
  })

  it('suggests the cheapest connected model for short questions and each family for its own work', () => {
    const routes = suggestRoutes(accounts)
    expect(routes.map((route) => route.key)).toEqual(['fast', 'openai', 'anthropic'])
    // GPT-5.6 nano at $0.05 undercuts Claude Haiku at $1.
    expect(routes[0]).toMatchObject({ family: 'openai', model: 'gpt-5.6-nano' })
    expect(routes[1]).toMatchObject({ family: 'openai', model: 'gpt-6-astra' })
    expect(routes[2]).toMatchObject({ family: 'anthropic', model: 'claude-opus-5' })
    expect(routes.every((route) => route.description.length > 20)).toBe(true)
    expect(suggestedFallback(routes)).toBe('fast')
  })

  it('suggests nothing while no account is enabled', () => {
    expect(suggestRoutes([])).toEqual([])
    expect(suggestRoutes([{ family: 'openai', enabled: false }])).toEqual([])
    expect(suggestedFallback([])).toBe('')
  })

  it('falls back to a family route when no connected family has a cheap model', () => {
    const routes = suggestRoutes([{ family: 'kimi', enabled: true }])
    expect(routes.map((route) => route.key)).toEqual(['kimi'])
    expect(routes[0].model).toBe('kimi-k3')
    expect(suggestedFallback(routes)).toBe('kimi')
  })
})
