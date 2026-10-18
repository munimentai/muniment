import { cubicOut } from 'svelte/easing'
import { motionReduced } from './display-state.js'

// The mark fades when its run settles. A mark that leaves with its thread while
// the run still goes is instant, so the old transcript never lingers.
export function thinkingSettle(_node, { instant = false } = {}) {
  const reducedMotion = motionReduced()
  return {
    duration: reducedMotion || instant ? 0 : 180,
    easing: cubicOut,
    css: (t) => `opacity: ${t}; transform: translateY(${(1 - t) * -2}px) scale(${0.96 + t * 0.04})`,
  }
}
