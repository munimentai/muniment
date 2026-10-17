import { render } from '@testing-library/svelte'
import { expect, it } from 'vitest'
import { avatarBounds, avatarFor, avatarSvg, newAvatar } from './agent-avatar.js'
import AgentAvatar from './AgentAvatar.svelte'
import { DIMS, svg, traits } from './muniment-avatar.js'
// Output of the brand kit's avatars/m/robots.js for these seeds, recorded from that file.
import fixtures from './muniment-avatar.fixtures.json'

it('draws exactly what the brand generator draws for every recorded seed', () => {
  expect(fixtures).toHaveLength(20)
  for (const { seed, svg: full, bare } of fixtures) {
    expect(svg(seed)).toBe(full)
    expect(svg(seed, { background: false, size: 64 })).toBe(bare)
  }
})

it('keeps the avatar across renames and draws every saved style from its seed', () => {
  const agent = { id: 'original-agent', name: 'Scout' }
  expect(avatarSvg(avatarFor(agent))).toBe(avatarSvg(avatarFor({ ...agent, name: 'Researcher' })))
  expect(avatarFor(agent).seed).toMatch(/^legacy-[0-9a-f]+$/)
  expect(avatarFor(agent).seed).not.toContain(agent.id)
  for (const style of ['muniment', 'muniment-v1', 'muniment-v2', 'unknown']) {
    expect(avatarFor({ ...agent, avatar: { style, seed: 'scout' } }).seed).toBe('scout')
    expect(avatarSvg({ style, seed: 'scout' })).toBe(avatarSvg({ style: 'muniment', seed: 'scout' }))
  }
  expect(avatarSvg({ seed: 'one' })).not.toBe(avatarSvg({ seed: 'two' }))
  const fresh = newAvatar()
  expect(fresh.style).toBe('muniment')
  expect(fresh.seed).not.toBe(newAvatar().seed)
})

it('wraps the brand drawing unchanged and never inserts the seed', () => {
  for (let i = 0; i < 200; i++) {
    const seed = `${i}<script>alert(1)</script>"`
    const markup = avatarSvg({ seed })
    expect(markup).toContain(svg(seed, { background: false }).replace(/^<svg[^>]*>/, '').replace(/<\/svg>$/, ''))
    expect(markup).not.toMatch(/<script|alert|href=|<image|<foreignObject|onload=|aria-label/)
    expect(markup).toMatch(/^<svg [^>]*viewBox="0 0 120 120" preserveAspectRatio="xMidYMid meet">/)
  }
})

it('centers each drawing on the tile and keeps it inside', () => {
  const seen = new Set()
  for (let i = 0; i < 2000; i++) {
    const seed = `center-${i}`
    const t = traits(seed)
    seen.add(`${t.head}:${t.top}:${t.sides}`)
    const box = avatarBounds(seed)
    const [, dx, dy] = avatarSvg({ seed }).match(/translate\(([-\d.]+) ([-\d.]+)\)/).map(Number)
    expect(Math.abs((box.left + box.right) / 2 + dx - 60)).toBeLessThan(0.06)
    expect(Math.abs((box.top + box.bottom) / 2 + dy - 60)).toBeLessThan(0.06)
    expect(box.left + dx).toBeGreaterThan(4)
    expect(box.top + dy).toBeGreaterThan(4)
  }
  expect(seen.size).toBe(DIMS.head.length * DIMS.top.length * DIMS.sides.length)
})

it('renders a decorative square box at the size it is given', () => {
  const { container } = render(AgentAvatar, { agent: { id: 'a1', name: 'Scout', avatar: { style: 'muniment', seed: 'scout' } }, size: 48 })
  const box = container.querySelector('.agent-avatar')
  expect(box.getAttribute('aria-hidden')).toBe('true')
  expect(box.style.width).toBe('48px')
  expect(box.style.height).toBe('48px')
  expect(box.querySelector('svg')).not.toBeNull()
  const template = document.createElement('template')
  template.innerHTML = avatarSvg({ seed: 'scout' })
  expect(box.querySelector('svg').isEqualNode(template.content.firstChild)).toBe(true)
})
