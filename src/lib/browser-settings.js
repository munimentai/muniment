export const DEFAULT_HOMEPAGE = 'https://muniment.ai/docs/'
const key = 'muniment.browser.homepage'
export function homepageUrl(value) {
  const text = value.trim()
  if (!text) return DEFAULT_HOMEPAGE
  const url = new URL(text.includes('://') ? text : `https://${text}`)
  if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password) throw new Error('Enter an HTTP or HTTPS homepage without sign-in details.')
  return url.href
}
export function readHomepage() {
  try { const saved = localStorage.getItem(key); return homepageUrl(!saved || saved === 'https://muniment.ai/' ? DEFAULT_HOMEPAGE : saved) }
  catch { return DEFAULT_HOMEPAGE }
}
export function saveHomepage(value) {
  const url = homepageUrl(value)
  localStorage.setItem(key, url)
  return url
}
