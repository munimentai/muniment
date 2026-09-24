import { expect, it, vi } from 'vitest'
import { changedFiles, createChangedFiles } from './file-changes.js'
import { HIGHLIGHT_LIMIT_BYTES, highlightFile } from './file-highlight.js'
it('counts applied hunks once and omits failed tool edits', () => {
  const applied = { effectId: 'one', diff: { files: [{ newPath: 'src/a.js', hunks: [{ lines: [{ kind: 'addition' }, { kind: 'deletion' }, { kind: 'context' }] }] }] } }
  expect(changedFiles([{ run: { appliedDiffs: [applied, applied], toolActivity: [{ displayName: 'write', status: 'failed', input: '{"path":"wrong"}' }] } }])).toMatchObject([{ path: 'src/a.js', additions: 1, deletions: 1 }])
})
it('does not fabricate counts for writes without an original snapshot', () => {
  expect(changedFiles([{ run: { toolActivity: [{ displayName: 'write', status: 'completed', input: '{"path":"a.txt","content":"hello"}' }] } }])[0].incomplete).toBe(true)
})
it('escapes markup and highlights known languages without evaluating content', () => {
  expect(highlightFile('a.txt', '<img onerror="evil()">')).toContain('&lt;img')
  expect(highlightFile('a.js', 'const name = "test"')).toContain('hljs-keyword')
  expect(highlightFile('a.html', '<script>alert(1)</script>')).not.toContain('<script>')
})
it('keeps the changed files while streamed text replaces the run', () => {
  const read = createChangedFiles()
  const appliedDiffs = [{ effectId: 'one', diff: { files: [{ newPath: 'src/a.js', hunks: [{ lines: [{ kind: 'addition' }] }] }] } }]
  const toolActivity = [{ displayName: 'edit', status: 'completed', input: '{"path":"src/b.js","oldText":"a","newText":"b\\nc"}' }]
  const parse = vi.spyOn(JSON, 'parse')
  const first = read([{ run: { text: 'A', appliedDiffs, toolActivity } }])
  expect(first).toMatchObject([{ path: 'src/a.js', additions: 1 }, { path: 'src/b.js', additions: 2, deletions: 1 }])
  expect(read([{ run: { text: 'A longer reply', appliedDiffs, toolActivity } }])).toBe(first)
  const next = read([{ run: { text: 'A longer reply', appliedDiffs, toolActivity: [...toolActivity] } }])
  expect(next).not.toBe(first)
  expect(next).toEqual(first)
  expect(parse).toHaveBeenCalledTimes(2)
  parse.mockRestore()
})
it('shows a file above the highlight limit as escaped plain text', () => {
  const large = `const a = "<b>"\n${'x'.repeat(HIGHLIGHT_LIMIT_BYTES)}`
  const result = highlightFile('a.js', large)
  expect(result).not.toContain('hljs-')
  expect(result.startsWith('const a = "&lt;b&gt;"')).toBe(true)
  expect(highlightFile('a.js', `const a = "${'é'.repeat(HIGHLIGHT_LIMIT_BYTES / 2)}"`)).not.toContain('hljs-')
  expect(highlightFile('a.js', `const a = "${'é'.repeat(HIGHLIGHT_LIMIT_BYTES / 4)}"`)).toContain('hljs-keyword')
})
