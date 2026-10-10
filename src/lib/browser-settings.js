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

// The address bar searches with this engine. `%s` takes the encoded words.
export const SEARCH_ENGINES = [
  { id: 'google', name: 'Google', url: 'https://www.google.com/search?q=%s' },
  { id: 'brave', name: 'Brave Search', url: 'https://search.brave.com/search?q=%s' },
  { id: 'duckduckgo', name: 'DuckDuckGo', url: 'https://duckduckgo.com/?q=%s' },
  { id: 'bing', name: 'Bing', url: 'https://www.bing.com/search?q=%s' },
  { id: 'startpage', name: 'Startpage', url: 'https://www.startpage.com/do/search?query=%s' },
]
export const DEFAULT_SEARCH = 'google'
const searchKey = 'muniment.browser.search'
export function readSearchEngine() {
  try { const saved = localStorage.getItem(searchKey); return SEARCH_ENGINES.some(engine => engine.id === saved) ? saved : DEFAULT_SEARCH }
  catch { return DEFAULT_SEARCH }
}
export function saveSearchEngine(id) {
  if (!SEARCH_ENGINES.some(engine => engine.id === id)) throw new Error('Choose a listed search engine.')
  localStorage.setItem(searchKey, id)
  return id
}

// Bangs search one site, as in Brave Search: `!w ada lovelace` or `ada lovelace !w`.
// A bang this table lacks goes to DuckDuckGo, which resolves thousands more.
export const BANGS = {
  g: 'https://www.google.com/search?q=%s',
  gi: 'https://www.google.com/search?tbm=isch&q=%s',
  m: 'https://www.google.com/maps/search/%s',
  maps: 'https://www.google.com/maps/search/%s',
  b: 'https://search.brave.com/search?q=%s',
  ddg: 'https://duckduckgo.com/?q=%s',
  bi: 'https://www.bing.com/search?q=%s',
  sp: 'https://www.startpage.com/do/search?query=%s',
  w: 'https://en.wikipedia.org/wiki/Special:Search?search=%s',
  yt: 'https://www.youtube.com/results?search_query=%s',
  gh: 'https://github.com/search?q=%s',
  r: 'https://www.reddit.com/search/?q=%s',
  so: 'https://stackoverflow.com/search?q=%s',
  mdn: 'https://developer.mozilla.org/en-US/search?q=%s',
  npm: 'https://www.npmjs.com/search?q=%s',
  pypi: 'https://pypi.org/search/?q=%s',
  crates: 'https://crates.io/search?q=%s',
  hn: 'https://hn.algolia.com/?q=%s',
  a: 'https://www.amazon.com/s?k=%s',
  ebay: 'https://www.ebay.com/sch/i.html?_nkw=%s',
  imdb: 'https://www.imdb.com/find/?q=%s',
  wa: 'https://www.wolframalpha.com/input?i=%s',
  tr: 'https://translate.google.com/?text=%s',
  x: 'https://x.com/search?q=%s',
}
const fill = (template, words) => words ? template.replace('%s', encodeURIComponent(words)) : new URL(template).origin + '/'
// Text with no spaces that ends in a domain, a host with a port, or localhost reads as an address.
const address = /^(?:localhost|\d{1,3}(?:\.\d{1,3}){3}|[^\s/:?#]+\.[a-z][a-z0-9-]+)(?::\d+)?(?:[/?#]\S*)?$/i
const local = /^(?:localhost|\d{1,3}(?:\.\d{1,3}){3})(?:[:/?#]|$)/i
export function addressFor(input, engine = readSearchEngine()) {
  const text = input.trim()
  if (!text) return null
  if (/^https?:\/\//i.test(text)) return text
  if (address.test(text)) return `${local.test(text) ? 'http' : 'https'}://${text}`
  const words = text.split(/\s+/)
  const bang = words.findIndex(word => /^![a-z0-9.]+$/i.test(word))
  if (bang !== -1) {
    const template = BANGS[words[bang].slice(1).toLowerCase()]
    if (template) return fill(template, words.filter((_, index) => index !== bang).join(' '))
    return fill(BANGS.ddg, text)
  }
  const search = SEARCH_ENGINES.find(entry => entry.id === engine) ?? SEARCH_ENGINES[0]
  return fill(search.url, text)
}
