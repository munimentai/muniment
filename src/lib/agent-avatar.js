// Agent avatars draw with the brand's MunimentAvatar generator from the agent's
// saved seed. Every saved style draws the same way, so an agent keeps one
// avatar for its seed.
import { svg as munimentSvg, traits as munimentTraits } from './muniment-avatar.js'

// The head boxes MunimentAvatar draws in its 120 unit view box, as [x, y, width, height].
const heads = {
  block: [30, 36, 60, 60],
  wide: [20, 44, 80, 52],
  tall: [34, 30, 52, 70],
  panel: [28, 36, 64, 62],
  arch: [28, 34, 64, 64],
  tablet: [30, 34, 60, 64],
}
// How far each top piece reaches above the head.
const topReach = { none: 0, fold: 0, stalk: 19, ball: 19, twin: 15, tab: 8 }
// How far the side pieces reach past the head's left and right edges.
const sideReach = { none: [0, 0], bolts: [5, 5], ears: [8, 8], tabs: [0, 5] }

function legacySeed(id) { let n = 2166136261; for (const c of id) n = Math.imul(n ^ c.charCodeAt(0), 16777619); return `legacy-${(n >>> 0).toString(16)}` }
export function avatarFor(agent) {
  const avatar = agent?.avatar
  return typeof avatar?.seed === 'string' && avatar.seed ? avatar : { style: 'muniment-v1', seed: legacySeed(agent?.id || 'new-agent') }
}
export function newAvatar() { return { style: 'muniment', seed: crypto.randomUUID() } }

// The box around everything an avatar draws on its tile, in view box units.
export function avatarBounds(seed) {
  const t = munimentTraits(String(seed))
  const [x, y, w, h] = heads[t.head]
  const [left, right] = sideReach[t.sides]
  return { left: x - left, right: x + w + right, top: y - topReach[t.top], bottom: y + h }
}

// The generator draws heads low in its view box, under room for a top piece.
// The avatar moves, never changes, so its bounds center on the tile.
export function avatarSvg(avatar) {
  const seed = String(avatar?.seed || 'new-agent')
  const { left, right, top, bottom } = avatarBounds(seed)
  const dx = Math.round((60 - (left + right) / 2) * 10) / 10
  const dy = Math.round((60 - (top + bottom) / 2) * 10) / 10
  const drawing = munimentSvg(seed, { background: false }).replace(/^<svg[^>]*>/, '').replace(/<\/svg>$/, '')
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120" preserveAspectRatio="xMidYMid meet"><rect width="120" height="120" fill="${munimentTraits(seed).bg}"/><g transform="translate(${dx} ${dy})">${drawing}</g></svg>`
}
