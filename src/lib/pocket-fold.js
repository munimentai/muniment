import { DEGREE, trunkPath } from './elephant-trunk.js'

// The pocket-fold symbol from brand/v2: the whole body as one shape, and the
// ear that folds over it.
export const FOLD_BODY = 'M160 0 H274 V118 A86 86 0 0 0 360 204 H514 A12 12 0 0 1 526 216 V423 A36 36 0 0 1 490 459 H437 A10 10 0 0 1 427 449 V338 A54 56 0 0 0 319 338 V449 A10 10 0 0 1 309 459 H249 A10 10 0 0 1 239 449 V338 A57 56 0 0 0 125 338 V449 A10 10 0 0 1 115 459 H36 A36 36 0 0 1 0 423 V140 A160 140 0 0 1 160 0 Z'
export const FOLD_EAR = 'M294 0 L510 190 H366 A72 72 0 0 1 294 118 Z'
export const FOLD_VIEWBOX = '-20 -20 566 499'

// The same symbol cut into the parts an elephant moves: the torso, the hind
// leg, the fore leg, the trunk and the ear. Each leg has a round cap and turns
// about the cap's center, so a joint stays a smooth curve at every angle. The
// trunk is one shape that bends along its length.
export const TORSO = 'M160 0 H274 V118 A86 86 0 0 0 360 204 H514 A12 12 0 0 1 526 216 V338 H427 A54 56 0 0 0 319 338 H239 A57 56 0 0 0 125 338 H0 V140 A160 140 0 0 1 160 0 Z'
export const HIND = 'M0 338 A62.5 62.5 0 0 1 125 338 V449 A10 10 0 0 1 115 459 H36 A36 36 0 0 1 0 423 Z'
// The fore leg grows up into the body as the front rises, so its foot stays down.
export const foreLeg = (rise) => `M239 ${338 - rise} A40 40 0 0 1 319 ${338 - rise} V449 A10 10 0 0 1 309 459 H249 A10 10 0 0 1 239 449 Z`
export const EAR = FOLD_EAR
// The front rises about the center of the hind leg's cap, so the hip stays a
// smooth joint. FORE_REACH is the distance from it to the fore leg's cap.
export const TILT_DEG = 8
export const HIP = [62.5, 338]
export const FORE_REACH = 216.5
// How far the fore leg grows at a given share of the full rise.
export const foreRise = (tilt) => Math.round(FORE_REACH * Math.sin(TILT_DEG * tilt * DEGREE) * 10) / 10

const ease = (t) => t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2
const COLORS = '.body{fill:#111111}.ear{fill:#2A7264}@media(prefers-color-scheme:dark){.body{fill:#FFFFFF}}'
// A square view around the symbol, so it sits centered in a square slot.
const SQUARE = '-20 -53.5 566 566'

// The mark the sign-in pages show, with the brand colors on a light or a dark
// page. The still mark is for a page that reports a failure.
export function signInMarkSvg() {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="56" height="56" viewBox="${SQUARE}" role="img" aria-label="muniment">`
    + `<style>${COLORS}</style><title>muniment</title><path class="body" d="${FOLD_BODY}"/><path class="ear" d="${FOLD_EAR}"/></svg>`
}

// The mark a finished sign-in shows: once, the elephant rears up, curls its
// trunk into the J, flaps its ear and settles back into the logo. SMIL plays it,
// since the page runs no script, and reduced motion shows the still mark.
export function signedInMarkSvg() {
  const BEGIN = '0.4s'
  const DUR = '2.8s'
  // Rise over the first quarter, hold through the middle half, settle in the last quarter.
  const STEPS = 8
  const rise = Array.from({ length: STEPS + 1 }, (_, i) => ease(i / STEPS))
  const curls = [...rise, ...[...rise].reverse()]
  const curlTimes = [...rise.map((_, i) => 0.25 * i / STEPS), ...rise.map((_, i) => 0.75 + 0.25 * i / STEPS)]
  const turn = (values, times, splines) => `values="${values.join(';')}" keyTimes="${times.join(';')}" calcMode="spline" keySplines="${splines.join(';')}" dur="${DUR}" begin="${BEGIN}" fill="freeze"`
  const EASE = '.42 0 .58 1'
  const HOLD = '0 0 1 1'
  const pose = (tilt) => `${-TILT_DEG * tilt} ${HIP[0]} ${HIP[1]}`
  const fold = 41.33
  const [earX, earY] = [402, 95]
  return `<svg xmlns="http://www.w3.org/2000/svg" width="56" height="56" viewBox="${SQUARE}" role="img" aria-label="muniment" style="overflow:visible">`
    + `<style>${COLORS}.still{display:none}@media(prefers-reduced-motion:reduce){.moving{display:none}.still{display:inline}}</style>`
    + '<title>muniment</title>'
    + `<g class="still"><path class="body" d="${FOLD_BODY}"/><path class="ear" d="${FOLD_EAR}"/></g>`
    + '<g class="moving">'
    + `<path class="body" d="${HIND}"/>`
    + `<path class="body" d="${foreLeg(0)}"><animate attributeName="d" ${turn([0, 1, 1, 0].map((tilt) => foreLeg(foreRise(tilt))), [0, 0.25, 0.75, 1], [EASE, HOLD, EASE])}/></path>`
    + `<g><animateTransform attributeName="transform" type="rotate" ${turn([0, 1, 1, 0].map(pose), [0, 0.25, 0.75, 1], [EASE, HOLD, EASE])}/>`
    + `<path class="body" d="${trunkPath()}"><animate attributeName="d" values="${curls.map((curl) => trunkPath(curl)).join(';')}" keyTimes="${curlTimes.map((time) => Math.round(time * 10000) / 10000).join(';')}" dur="${DUR}" begin="${BEGIN}" fill="freeze"/></path>`
    + `<path class="body" d="${TORSO}"/>`
    // The ear hinges on its diagonal edge: it unfolds slowly, whips out, and falls back.
    + `<g transform="translate(${earX} ${earY}) rotate(${fold})"><g><animateTransform attributeName="transform" type="scale" ${turn(['1 1', '1 1', '1 -.6', '1 1', '1 1'], [0, 0.35, 0.5, 0.62, 1], [HOLD, '.7 0 .84 0', '.16 1 .3 1', HOLD])}/>`
    + `<g transform="rotate(${-fold}) translate(${-earX} ${-earY})"><path class="ear" d="${EAR}"/></g></g></g>`
    + '</g></g></svg>'
}
