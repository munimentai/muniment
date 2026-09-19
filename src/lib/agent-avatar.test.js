import { expect, it } from 'vitest'
import { avatarFor, avatarSvg } from './agent-avatar.js'
it('keeps the avatar across renames and changes it when its saved seed changes', () => {
  const agent = { id: 'original-agent', name: 'Scout' }
  expect(avatarSvg(avatarFor(agent))).toBe(avatarSvg(avatarFor({ ...agent, name: 'Researcher' })))
  expect(avatarSvg({ seed: 'one' })).not.toBe(avatarSvg({ seed: 'two' }))
  expect(avatarFor(agent).seed).not.toContain(agent.id)
})
it('renders compact local SVGs from only fixed geometry and neutral or happy expressions', () => {
  for (let i = 0; i < 100; i++) {
    const svg = avatarSvg({ seed: `${i}<script>alert(1)</script>` })
    expect(svg.length).toBeLessThan(1000)
    expect(svg).not.toMatch(/<script|href=|<image|<foreignObject|onload=/)
    expect(svg).toMatch(/M43 65q7 6 14 0|M46 66h8/)
  }
})
it('generates many distinct combinations with rare frowns and kisses', async () => {
  const { avatarTraits, newAvatar } = await import('./agent-avatar.js')
  expect(newAvatar().style).toBe('muniment-v2')
  const shapes = new Set()
  const counts = {}
  for (let i = 0; i < 10000; i++) {
    const avatar = { style: 'muniment-v2', seed: `variety-${i}` }
    const traits = avatarTraits(avatar)
    counts[traits.mouth] = (counts[traits.mouth] || 0) + 1
    const svg = avatarSvg(avatar)
    shapes.add(svg)
    expect(svg.length).toBeLessThan(1800)
    expect(svg).not.toMatch(/<script|href=|<image|<foreignObject|onload=/)
  }
  expect(shapes.size).toBeGreaterThan(9000)
  for (const rare of ['frown', 'kiss']) {
    expect(counts[rare]).toBeGreaterThan(5)
    expect(counts[rare]).toBeLessThan(40)
  }
  expect(counts.teeth).toBeGreaterThan(1500)
  const avatar = { style: 'muniment-v2', seed: 'stable' }
  expect(avatarFor({ avatar })).toEqual(avatar)
  expect(avatarSvg(avatar)).toBe(avatarSvg(avatarFor({ avatar, name: 'Renamed' })))
})
