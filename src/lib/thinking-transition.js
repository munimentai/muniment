import { cubicOut } from 'svelte/easing'

export function thinkingSettle() {
  const reducedMotion = window.matchMedia?.('(prefers-reduced-motion: reduce)')?.matches ?? false
  return {
    duration: reducedMotion ? 0 : 180,
    easing: cubicOut,
    css: (t) => `opacity: ${t}; transform: translateY(${(1 - t) * -2}px) scale(${0.96 + t * 0.04})`,
  }
}
