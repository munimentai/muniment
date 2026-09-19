// Original muniment artwork. Geometry is local, deterministic and versioned.
const colors = ['#EF956B', '#E8BC58', '#79B8A0', '#80A9DE', '#B49ADA', '#D78EA7', '#91B5C0', '#B3BD75']
const bodies = [
  '<path d="M18 57C12 28 29 13 51 15c27 1 42 23 34 47-6 20-22 27-44 22C27 81 21 73 18 57Z"/>',
  '<path d="M19 66c-10-11-7-29 6-33 0-19 26-29 39-13 17-4 30 13 23 28 15 21-5 39-22 35-17 12-37 2-36-9-4 0-8-3-10-8Z"/>',
  '<path d="M37 18c6-9 19-9 25 0l25 45c8 15 0 23-16 23H29c-16 0-24-8-16-23Z"/>',
  '<path d="M16 41c0-19 9-27 34-27s34 8 34 27v20c0 19-9 26-34 26s-34-7-34-26Z"/>',
  '<path d="M17 54c-5-17 3-30 18-30 5-16 27-15 34-1 19-2 29 20 18 32 5 23-20 37-35 27-19 13-41-9-35-28Z"/>',
  '<path d="M17 63c-7-22 6-43 31-47 27-4 39 13 36 36-2 19-18 35-40 34-14-1-23-9-27-23Z"/>',
]
function random(seed) {
  let n = 2166136261
  for (const c of seed) n = Math.imul(n ^ c.charCodeAt(0), 16777619)
  return () => { n += 0x6D2B79F5; let t = Math.imul(n ^ n >>> 15, n | 1); t ^= t + Math.imul(t ^ t >>> 7, t | 61); return ((t ^ t >>> 14) >>> 0) / 4294967296 }
}
function legacySeed(id) { let n = 2166136261; for (const c of id) n = Math.imul(n ^ c.charCodeAt(0), 16777619); return `legacy-${(n >>> 0).toString(16)}` }
export function avatarFor(agent) {
  return ['muniment-v1', 'muniment-v2'].includes(agent.avatar?.style) ? agent.avatar : { style: 'muniment-v1', seed: legacySeed(agent.id || 'new-agent') }
}
export function newAvatar() { return { style: 'muniment-v2', seed: crypto.randomUUID() } }
export function avatarSvg(avatar) {
  if (avatar?.style === 'muniment-v2') return variedAvatarSvg(avatar)
  const next = random(String(avatar?.seed || 'new-agent'))
  const body = bodies[Math.floor(next() * bodies.length)]
  const color = colors[Math.floor(next() * colors.length)]
  const tilt = Math.floor(next() * 13) - 6
  const eyes = Math.floor(next() * 4)
  const face = eyes === 0 ? '<path d="M34 47v8m26-8v8" stroke-width="7"/>'
    : eyes === 1 ? '<path d="M31 53q4-7 8 0m18 0q4-7 8 0" stroke-width="5"/>'
    : eyes === 2 ? '<path d="M33 48v7m24-3h9" stroke-width="6"/>'
    : '<path d="M33 50h1m28 0h1" stroke-width="8"/>'
  const mouth = next() < .5 ? '<path d="M43 65q7 6 14 0" stroke-width="3"/>' : '<path d="M46 66h8" stroke-width="3"/>'
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><g fill="${color}" transform="rotate(${tilt} 50 50)">${body}<g fill="none" stroke="#243330" stroke-linecap="round" stroke-linejoin="round">${face}${mouth}</g></g></svg>`
}
export const avatarDataUri = avatar => `data:image/svg+xml,${encodeURIComponent(avatarSvg(avatar))}`

const variedColors = [...colors, '#DEA267', '#A8C8A1', '#CCADE0', '#E9A4A0']
const eyeStyles = ['open', 'happy', 'wink', 'dots', 'stars', 'sparkle', 'crescent', 'wide']
export function avatarTraits(avatar) {
  const next = random(String(avatar?.seed || 'new-agent'))
  const body = Math.floor(next() * bodies.length)
  const color = variedColors[Math.floor(next() * variedColors.length)]
  const tilt = Math.floor(next() * 13) - 6
  const eyes = eyeStyles[Math.floor(next() * eyeStyles.length)]
  const roll = Math.floor(next() * 1000)
  const mouth = roll < 2 ? 'frown' : roll < 4 ? 'kiss' : roll < 204 ? 'teeth' : roll < 354 ? 'laugh' : roll < 754 ? 'smile' : roll < 904 ? 'neutral' : 'smirk'
  const frames = next()
  const glasses = ['stars', 'sparkle'].includes(eyes) ? 'none' : frames < .16 ? 'round' : frames < .29 ? 'square' : frames < .36 ? 'sun' : 'none'
  const cheeks = Math.floor(next() * 4)
  const faceY = Math.floor(next() * 5) - 2
  return { body, color, tilt, eyes, mouth, glasses, cheeks, faceY }
}
function variedAvatarSvg(avatar) {
  const t = avatarTraits(avatar)
  const star = '<path d="m0-8 2.5 5.2 5.7.8-4.1 4 1 5.7L0 5l-5.1 2.7 1-5.7-4.1-4 5.7-.8Z" fill="#FFF0AF" stroke-width="1.5"/>'
  const eyes = {
    open: '<path d="M35 46v9m25-9v9" stroke-width="6"/>',
    happy: '<path d="M30 52q5-9 10 0m15 0q5-9 10 0" stroke-width="4"/>',
    wink: '<path d="M34 47v8m22-3q5-4 10 0" stroke-width="5"/>',
    dots: '<path d="M34 50h1m27 0h1" stroke-width="7"/>',
    stars: `<g transform="translate(34 50)">${star}</g><g transform="translate(62 50)">${star}</g>`,
    sparkle: '<path d="m34 42 2 6 6 2-6 2-2 6-2-6-6-2 6-2Zm28 0 2 6 6 2-6 2-2 6-2-6-6-2 6-2Z" fill="#FFF0AF" stroke-width="1.5"/>',
    crescent: '<path d="M30 47q5 8 10 0m15 0q5 8 10 0" stroke-width="4"/>',
    wide: '<ellipse cx="34" cy="50" rx="5" ry="7" fill="#243330" stroke="none"/><ellipse cx="62" cy="50" rx="5" ry="7" fill="#243330" stroke="none"/><path d="M33 47h.1m28 0h.1" stroke="#FFF8E8" stroke-width="3"/>',
  }[t.eyes]
  const mouth = {
    neutral: '<path d="M45 67h10" stroke-width="3"/>',
    smile: '<path d="M40 64q10 13 20 0" stroke-width="3"/>',
    smirk: '<path d="M43 69q12 2 16-7" stroke-width="3"/>',
    teeth: '<path d="M37 63q13 4 26 0c-1 17-25 17-26 0Z" fill="#FFF8E8" stroke-width="2.5"/><path d="M39 68h22m-13-4v5m6-5v5" stroke-width="1.5"/>',
    laugh: '<path d="M40 63h20c0 18-20 18-20 0Z" fill="#243330" stroke-width="2"/><path d="M44 72q6-5 12 0" stroke="#EAA1A0" stroke-width="4"/>',
    frown: '<path d="M41 71q9-10 18 0" stroke-width="3"/>',
    kiss: '<path d="M47 62q13 2 3 6 10 5-3 7" stroke-width="3"/>',
  }[t.mouth]
  const glasses = {
    none: '',
    round: '<g stroke-width="3"><circle cx="34" cy="50" r="10"/><circle cx="62" cy="50" r="10"/><path d="M44 49h8m-28-2-5-2m53 2 5-2"/></g>',
    square: '<g stroke-width="3"><rect x="23" y="40" width="21" height="20" rx="5"/><rect x="52" y="40" width="21" height="20" rx="5"/><path d="M44 48h8m-29-2h-5m55 0h5"/></g>',
    sun: '<path d="M23 42h21v10c0 12-21 12-21 0Zm29 0h21v10c0 12-21 12-21 0Z" fill="#243330" stroke-width="2"/><path d="M44 47h8m-29-3h-5m55 0h5" stroke-width="3"/><path d="m27 46 7-1m23 1 7-1" stroke="#FFF8E8" stroke-width="2"/>',
  }[t.glasses]
  const cheeks = t.cheeks === 1 ? '<path d="M25 64h4m40 0h4" stroke="#CB6E79" opacity=".55" stroke-width="5"/>'
    : t.cheeks === 2 ? '<path d="M24 64h.1m5 1h.1m-2-5h.1m42 4h.1m5 1h.1m-2-5h.1" opacity=".45" stroke-width="2"/>' : ''
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><g fill="${t.color}" transform="rotate(${t.tilt} 50 50)">${bodies[t.body]}<g transform="translate(0 ${t.faceY})" fill="none" stroke="#243330" stroke-linecap="round" stroke-linejoin="round">${eyes}${cheeks}${mouth}${glasses}</g></g></svg>`
}
