export function mentionQuery(text, cursor = text.length) {
  const before = text.slice(0, cursor)
  const match = /(?:^|\s)@([^@\s]*)$/.exec(before)
  return match ? { query: match[1], start: before.length - match[1].length - 1, end: cursor } : null
}
export function insertMention(text, mention, path) {
  const token = `@${path} `
  return { text: text.slice(0, mention.start) + token + text.slice(mention.end), cursor: mention.start + token.length }
}
export function composerParts(text, references = []) {
  const escaped = references.filter(Boolean).sort((a, b) => b.length - a.length).map((name) => '@' + name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
  const expression = new RegExp('https?:\\/\\/[^\\s<>]+|' + [...escaped, '@[^\\s<>]+'].join('|'), 'g')
  const parts = []
  let start = 0
  for (const match of text.matchAll(expression)) {
    if (match.index > start) parts.push({ text: text.slice(start, match.index), type: 'text' })
    parts.push({ text: match[0], type: match[0].startsWith('@') ? 'file' : 'link' })
    start = match.index + match[0].length
  }
  parts.push({ text: text.slice(start), type: 'text' })
  return parts
}
