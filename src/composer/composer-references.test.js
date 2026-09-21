import { expect, it } from 'vitest'
import { mentionQuery, insertMention, composerParts } from './composer-references.js'
it('recognizes file search at the cursor without treating email as a mention', () => {
  expect(mentionQuery('Read @src/iss')).toEqual({ query: 'src/iss', start: 5, end: 13 })
  expect(mentionQuery('mail user@example.com')).toBeNull()
  expect(mentionQuery('Read @src/a.js ')).toBeNull()
})
it('inserts a file reference without discarding text after the cursor', () => {
  expect(insertMention('Read @iss please', { start: 5, end: 9 }, 'Project files/ISSUES.md').text).toBe('Read @Project files/ISSUES.md  please')
})
it('finds http links and file references while leaving markup as plain text', () => {
  const parts = composerParts('<script> https://pi.dev/packages?name=compact @ISSUES.md')
  expect(parts.filter((p) => p.type !== 'text').map((p) => p.type)).toEqual(['link', 'file'])
  expect(parts[0].text).toContain('<script>')
})

it('keeps file names with spaces as a single unquoted reference', () => {
  const text = insertMention('@proj', { start: 0, end: 5 }, 'Project files/ISSUES.md').text
  expect(text).toBe('@Project files/ISSUES.md ')
  expect(composerParts(text, ['Project files/ISSUES.md'])[0]).toEqual({ type: 'file', text: '@Project files/ISSUES.md' })
})

it('styles installed slash commands like URLs without matching path fragments', () => {
  const parts = composerParts('/review Check https://example.com/review and /Users/review', [], ['review'])
  expect(parts.filter(part => part.type === 'link').map(part => part.text)).toEqual(['/review', 'https://example.com/review'])
})
