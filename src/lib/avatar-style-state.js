export const AVATAR_STYLE_STORAGE_KEY = 'muniment.agent-avatars'
export const AVATAR_STYLE_EVENT = 'muniment:agent-avatars'

// How every agent avatar draws on this device: the brand's colorful robots or
// its muniment heads. An agent keeps only its seed, so the pick changes every
// avatar on screen and no agent.
export const AVATAR_STYLE_OPTIONS = [
  ['colorful', 'Colorful'],
  ['muniment', 'Muniment'],
]
export const DEFAULT_AVATAR_STYLE = 'colorful'

export function parseAvatarStyle(stored) {
  return AVATAR_STYLE_OPTIONS.some(([id]) => id === stored) ? stored : DEFAULT_AVATAR_STYLE
}

export function readStoredAvatarStyle() {
  try {
    return parseAvatarStyle(localStorage.getItem(AVATAR_STYLE_STORAGE_KEY))
  } catch (_) {
    return DEFAULT_AVATAR_STYLE
  }
}

// Stores and announces one change, so every avatar on screen follows it.
export function commitAvatarStyle(style) {
  const next = parseAvatarStyle(style)
  try { localStorage.setItem(AVATAR_STYLE_STORAGE_KEY, next) } catch (_) {}
  try { window.dispatchEvent(new CustomEvent(AVATAR_STYLE_EVENT, { detail: next })) } catch (_) {}
  return next
}
