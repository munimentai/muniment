import fs from 'node:fs'
import path from 'node:path'
import { render } from '@testing-library/svelte'
import { flushSync } from 'svelte'
import { describe, expect, it } from 'vitest'
import { agentColor, avatarBounds, avatarFor, avatarSvg, huedHead, newAvatar } from './agent-avatar.js'
import AgentAvatar from './AgentAvatar.svelte'
import * as colorful from './colorful-avatar.js'
import * as muniment from './muniment-avatar.js'
import { commitAvatarStyle, readStoredAvatarStyle } from './avatar-style-state.js'
import { documentTone } from './avatar-display.svelte.js'
// Output of the brand kit's avatars/robots.js and avatars/m/robots.js for these
// seeds, in both tones, with and without the tile, recorded from those files.
import fixtures from './avatar-generators.fixtures.json'

const generators = { colorful, muniment }

describe('the vendored brand generators', () => {
  it('draw exactly what the brand files draw for every recorded seed, tone and mode', () => {
    for (const [style, generator] of Object.entries(generators)) {
      expect(fixtures[style]).toHaveLength(12)
      for (const entry of fixtures[style]) {
        for (const tone of ['light', 'dark']) {
          expect(generator.svg(entry.seed, { tone })).toBe(entry[tone])
          expect(generator.svg(entry.seed, { tone, background: false, size: 64 })).toBe(entry[`${tone}Head`])
        }
      }
    }
  })

  // Set MUNIMENT_BRAND_AVATARS to the brand kit's avatars folder to compare the source too.
  it.skipIf(!process.env.MUNIMENT_BRAND_AVATARS)('keep the brand files\' code line for line', () => {
    for (const [file, vendored] of [['robots.js', 'colorful-avatar.js'], ['m/robots.js', 'muniment-avatar.js']]) {
      const brand = fs.readFileSync(path.join(process.env.MUNIMENT_BRAND_AVATARS, file), 'utf8').split('\n')
      const body = brand.slice(brand.findIndex((line) => line.startsWith('(function (root)')) + 2, brand.findIndex((line) => line.trim().startsWith('const api =')))
      expect(fs.readFileSync(path.join(process.cwd(), 'src/lib', vendored), 'utf8')).toContain(body.join('\n').trimEnd())
    }
  })
})

describe('agent avatars', () => {
  it('keep the avatar across renames and draw every saved style id from its seed', () => {
    const agent = { id: 'original-agent', name: 'Scout' }
    expect(avatarSvg(avatarFor(agent))).toBe(avatarSvg(avatarFor({ ...agent, name: 'Researcher' })))
    expect(avatarFor(agent).seed).toMatch(/^legacy-[0-9a-f]+$/)
    expect(avatarFor(agent).seed).not.toContain(agent.id)
    for (const saved of ['muniment', 'muniment-v1', 'muniment-v2', 'unknown']) {
      expect(avatarFor({ ...agent, avatar: { style: saved, seed: 'scout' } }).seed).toBe('scout')
      for (const style of ['colorful', 'muniment']) {
        expect(avatarSvg({ style: saved, seed: 'scout' }, { style })).toBe(avatarSvg({ seed: 'scout' }, { style }))
      }
    }
    expect(avatarSvg({ seed: 'one' })).not.toBe(avatarSvg({ seed: 'two' }))
    expect(newAvatar().style).toBe('muniment')
    expect(newAvatar().seed).not.toBe(newAvatar().seed)
  })

  it('draw the head alone in the picked style and tone, and never insert the seed', () => {
    for (const [style, generator] of Object.entries(generators)) {
      for (const tone of ['light', 'dark']) {
        for (let i = 0; i < 100; i++) {
          const seed = `${i}<script>alert(1)</script>"`
          const markup = avatarSvg({ seed }, { style, tone })
          const head = generator.svg(seed, { tone, background: false })
          expect(markup).toContain((style === 'colorful' ? huedHead(head, seed) : head).replace(/^<svg[^>]*>/, '').replace(/<\/svg>$/, ''))
          expect(markup).not.toMatch(/<script|alert|href=|<image|<foreignObject|onload=|aria-label|<rect width="120"/)
          expect(markup).toMatch(new RegExp(`^<svg [^>]*viewBox="0 0 120 120" preserveAspectRatio="xMidYMid meet" data-style="${style}" data-tone="${tone}">`))
        }
      }
    }
    expect(avatarSvg({ seed: 'scout' }, { style: 'muniment', tone: 'light' })).not.toBe(avatarSvg({ seed: 'scout' }, { style: 'muniment', tone: 'dark' }))
    expect(avatarSvg({ seed: 'scout' }, { style: 'nope', tone: 'nope' })).toBe(avatarSvg({ seed: 'scout' }, { style: 'colorful', tone: 'dark' }))
  })

  it('give every agent a hued color and draw a neutral colorful head in it', () => {
    const neutral = ['#ffffff', '#ece6d6']
    let recolored = 0
    for (let i = 0; i < 400; i++) {
      const seed = `hue-${i}`
      const color = agentColor({ avatar: { seed } })
      expect(neutral).not.toContain(color)
      expect(agentColor({ avatar: { style: 'muniment', seed } })).toBe(color)
      const fill = colorful.traits(seed).fill
      if (neutral.includes(fill)) {
        recolored++
        const markup = avatarSvg({ seed }, { style: 'colorful' })
        expect(markup).not.toMatch(new RegExp(`(fill|stroke)="${fill}"`))
        expect(markup).toContain(`="${color}"`)
      } else {
        expect(color).toBe(fill)
      }
    }
    expect(recolored).toBeGreaterThan(0)
  })

  it('center each head across the box and keep it inside', () => {
    for (const [style, generator] of Object.entries(generators)) {
      for (const tone of ['light', 'dark']) {
        const seen = new Set()
        for (let i = 0; i < 1500; i++) {
          const seed = `center-${i}`
          const t = generator.traits(seed)
          seen.add(t.sides)
          const box = avatarBounds(seed, style, tone)
          const dx = Number(avatarSvg({ seed }, { style, tone }).match(/<g transform="translate\(([-\d.]+) 0\)">/)[1])
          expect(Math.abs((box.left + box.right) / 2 + dx - 60)).toBeLessThan(0.06)
          expect(box.left + dx).toBeGreaterThan(1)
          expect(box.right + dx).toBeLessThan(119)
        }
        expect(seen.size).toBe(generator.DIMS.sides.length)
      }
    }
  })
})

describe('the avatar style and tone', () => {
  it('default to colorful and keep a pick on the device', () => {
    localStorage.removeItem('muniment.agent-avatars')
    expect(readStoredAvatarStyle()).toBe('colorful')
    expect(commitAvatarStyle('muniment')).toBe('muniment')
    expect(readStoredAvatarStyle()).toBe('muniment')
    expect(commitAvatarStyle('boxbot')).toBe('colorful')
  })

  it('read the tone from the theme on the root', () => {
    const root = document.createElement('div')
    root.dataset.theme = 'harbor-vibrant-light'
    expect(documentTone(root)).toBe('light')
    root.dataset.theme = 'moss-dark'
    expect(documentTone(root)).toBe('dark')
  })

  it('render one decorative box that redraws when the style or theme changes', async () => {
    commitAvatarStyle('colorful')
    document.documentElement.dataset.theme = 'moss-light'
    const { container } = render(AgentAvatar, { agent: { id: 'a1', name: 'Scout', avatar: { style: 'muniment', seed: 'scout' } }, size: 48 })
    const box = container.querySelector('.agent-avatar')
    expect(box.getAttribute('aria-hidden')).toBe('true')
    expect(box.style.width).toBe('48px')
    expect(box.style.height).toBe('48px')
    await Promise.resolve()
    flushSync()
    expect(box.querySelector('svg').dataset).toMatchObject({ style: 'colorful', tone: 'light' })
    commitAvatarStyle('muniment')
    document.documentElement.dataset.theme = 'moss-dark'
    await new Promise((resolve) => setTimeout(resolve, 0))
    flushSync()
    expect(box.querySelector('svg').dataset).toMatchObject({ style: 'muniment', tone: 'dark' })
    const template = document.createElement('template')
    template.innerHTML = avatarSvg({ seed: 'scout' }, { style: 'muniment', tone: 'dark' })
    expect(box.querySelector('svg').isEqualNode(template.content.firstChild)).toBe(true)
  })

  it('outline the head with the agent\'s activity and name it on hover', () => {
    const agent = { id: 'a1', name: 'Scout', avatar: { seed: 'scout' } }
    const states = [
      [undefined, null, null],
      [{ state: 'idle', action: null, since: null }, 'idle', 'Idle'],
      [{ state: 'running', action: 'Runs a scheduled routine' }, 'running', 'Running: Runs a scheduled routine'],
      [{ state: 'waiting', action: 'Run `git push origin main` in work' }, 'waiting', 'Waiting for you: Run git push origin main in work'],
      [{ state: 'error', action: 'The model could not complete this reply.' }, 'error', 'Last run failed: The model could not complete this reply.'],
    ]
    for (const [activity, state, title] of states) {
      const { container } = render(AgentAvatar, { agent, activity, size: 20 })
      const box = container.querySelector('.agent-avatar')
      expect(box.getAttribute('data-activity')).toBe(state)
      expect(box.getAttribute('data-tooltip')).toBe(title)
    }
  })
})
