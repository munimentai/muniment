import { expect, it } from 'vitest'
import { changedFiles } from './file-changes.js'
import { highlightFile } from './file-highlight.js'
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
