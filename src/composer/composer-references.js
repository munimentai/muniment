export function mentionQuery(text, cursor = text.length) {
  const before = text.slice(0, cursor)
  const match = /(?:^|\s)@([^@\s]*)$/.exec(before)
  return match ? { query: match[1], start: before.length - match[1].length - 1, end: cursor } : null
}
export function insertMention(text, mention, path) {
  const token = `@${path} `
  return { text: text.slice(0, mention.start) + token + text.slice(mention.end), cursor: mention.start + token.length }
}
const escape = (value) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
// The draft split into plain text, links, @file references, @agent mentions and /commands.
// Each part carries its `start` offset in the draft.
export function composerParts(text, references = [], commands = [], agents = []) {
  const names = (list) => list.filter(Boolean).sort((a, b) => b.length - a.length).map((name) => '@' + escape(name))
  const agentNames = new Set(agents)
  const commandTokens = commands.map(name => '(?<!\\S)/' + escape(name) + '(?=\\s|$)')
  const expression = new RegExp('https?:\\/\\/[^\\s<>]+|' + [...names([...references, ...agents]), ...commandTokens, '@[^\\s<>]+'].join('|'), 'g')
  const parts = []
  let start = 0
  for (const match of text.matchAll(expression)) {
    if (match.index > start) parts.push({ text: text.slice(start, match.index), type: 'text', start })
    const token = match[0]
    const type = token.startsWith('/') ? 'command' : !token.startsWith('@') ? 'link' : agentNames.has(token.slice(1)) ? 'agent' : 'file'
    parts.push({ text: token, type, start: match.index })
    start = match.index + token.length
  }
  parts.push({ text: text.slice(start), type: 'text', start })
  return parts
}
// The leading characters of a part that its icon covers: a link's scheme, or the @ of a reference.
export function referencePrefix(part) {
  if (part.type === 'link') return /^https?:\/\//.exec(part.text)[0]
  return part.type === 'file' || part.type === 'agent' ? '@' : ''
}
