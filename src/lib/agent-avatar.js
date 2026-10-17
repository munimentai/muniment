// Agent avatars draw with one of the brand's two generators, picked on this
// device: the colorful robots (`colorful-avatar.js`) or the muniment heads
// (`muniment-avatar.js`). An agent keeps only its seed. Every saved style id
// draws the picked style from that seed, so an agent keeps one avatar per
// style. Each draws its head alone, with no tile, in the tone of the theme.
import { svg as colorfulSvg, traits as colorfulTraits } from './colorful-avatar.js'
import { svg as munimentSvg, traits as munimentTraits, THEMES as MUNIMENT_THEMES } from './muniment-avatar.js'

export const AVATAR_STYLES = ['colorful', 'muniment']

function legacySeed(id) { let n = 2166136261; for (const c of id) n = Math.imul(n ^ c.charCodeAt(0), 16777619); return `legacy-${(n >>> 0).toString(16)}` }
export function avatarFor(agent) {
  const avatar = agent?.avatar
  return typeof avatar?.seed === 'string' && avatar.seed ? avatar : { style: 'muniment-v1', seed: legacySeed(agent?.id || 'new-agent') }
}
// Core checks the style id, so a new avatar keeps the id it accepts. Drawing
// never reads it.
export function newAvatar() { return { style: 'muniment', seed: crypto.randomUUID() } }

const style = (value) => AVATAR_STYLES.includes(value) ? value : 'colorful'
const tone = (value) => value === 'light' ? 'light' : 'dark'

// [x, y, width, height] of each head in the 120 unit view box.
const COLORFUL_HEADS = {
  square: [28, 36, 64, 64], wide: [20, 44, 80, 54], tall: [34, 30, 52, 72], rounded: [28, 36, 64, 64],
  chamfer: [28, 36, 64, 64], dome: [28, 36, 64, 64], trapezoid: [26, 38, 68, 62],
}
const MUNIMENT_HEADS = {
  block: [30, 36, 60, 60], wide: [20, 44, 80, 52], tall: [34, 30, 52, 70],
  panel: [28, 36, 64, 62], arch: [28, 34, 64, 64], tablet: [30, 34, 60, 64],
}

// How far the drawing reaches past the head's left and right edges: strokes,
// side pieces, flourishes and the shadow, which falls to the right.
function reach(kind, seed, mode) {
  if (kind === 'muniment') {
    const t = munimentTraits(seed)
    const edge = MUNIMENT_THEMES.find((theme) => theme.name === t.theme)[mode].stroke ? 1.5 : 0
    const sides = { none: [0, 0], bolts: [5 + edge, 5 + edge], ears: [8 + edge, 8 + edge], tabs: [0, 5] }[t.sides]
    return { head: MUNIMENT_HEADS[t.head], left: Math.max(edge, sides[0]), right: Math.max(4, edge, sides[1]) }
  }
  const t = colorfulTraits(seed)
  const inset = t.head === 'trapezoid' ? 4 : 0
  const side = { none: 0, bolts: 8.5 - inset, ears: 11.5 - inset, phones: 12.5 - inset }[t.sides]
  const sparkle = t.extra === 'sparkle' ? (t.sides === 'none' ? 18 : 15.5) : 0
  const both = Math.max(2, side, sparkle)
  return { head: COLORFUL_HEADS[t.head], left: both, right: Math.max(5, both) }
}

function drawing(kind, seed, mode) {
  const draw = kind === 'muniment' ? munimentSvg : colorfulSvg
  return draw(seed, { tone: mode, background: false })
}

// The box around everything a head draws, in view box units, after the
// generator's own vertical centering.
export function avatarBounds(seed, kind = 'colorful', mode = 'dark') {
  const picked = style(kind)
  const { head: [x, , w], left, right } = reach(picked, String(seed), tone(mode))
  const markup = drawing(picked, String(seed), tone(mode))
  const dy = Number(markup.match(/<g transform="translate\(0 ([-\d.]+)\)">/)?.[1] ?? 0)
  return { left: x - left, right: x + w + right, dy }
}

// One head alone, in the picked style and the theme's tone. The generator
// centers it vertically. The head moves, never changes, so its left and right
// bounds also center on the box. The generator's outer element, with the seed
// in its label, is dropped.
export function avatarSvg(avatar, { style: kind = 'colorful', tone: mode = 'dark' } = {}) {
  const seed = String(avatar?.seed || 'new-agent')
  const picked = style(kind)
  const { left, right } = avatarBounds(seed, picked, mode)
  const dx = Math.round((60 - (left + right) / 2) * 10) / 10
  const inner = drawing(picked, seed, tone(mode)).replace(/^<svg[^>]*>/, '').replace(/<\/svg>$/, '')
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120" preserveAspectRatio="xMidYMid meet" data-style="${picked}" data-tone="${tone(mode)}"><g transform="translate(${dx} 0)">${inner}</g></svg>`
}
