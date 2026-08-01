import { describe, expect, it } from 'vitest'
import { html } from 'diff2html'
import { codeDiffPresentation, codeDiffRendererOptions } from './code-diff.js'

const textDiff = {
  schemaVersion: 1,
  id: 'diff-1',
  truncated: false,
  files: [{
    oldPath: 'src/old.js',
    newPath: 'src/new.js',
    oldMode: '100644',
    newMode: '100755',
    status: 'renamed',
    binary: false,
    hunks: [{
      oldStart: 7,
      oldCount: 2,
      newStart: 7,
      newCount: 2,
      header: '@@ -7,2 +7,2 @@ function name()',
      lines: [
        { kind: 'context', oldLineNumber: 7, newLineNumber: 7, text: 'const fixed = true', segments: [] },
        { kind: 'deletion', oldLineNumber: 8, text: 'const name = "old"', segments: [] },
        { kind: 'addition', newLineNumber: 8, text: 'const name = "new"', segments: [] },
      ],
    }],
  }],
}

describe('code diff presentation', () => {
  it('adapts text files into bounded side-by-side renderer input', () => {
    const presentation = codeDiffPresentation(textDiff)

    expect(presentation.id).toBe('diff-1')
    expect(presentation.renderer.options).toBe(codeDiffRendererOptions)
    expect(presentation.renderer.options).toMatchObject({
      outputFormat: 'side-by-side',
      matchingMaxComparisons: 2500,
      maxLineSizeInBlockForComparison: 200,
      maxLineLengthHighlight: 10000,
    })
    expect(presentation.renderer.input).toEqual([expect.objectContaining({
      oldName: 'src/old.js',
      newName: 'src/new.js',
      oldMode: '100644',
      newMode: '100755',
      addedLines: 1,
      deletedLines: 1,
      isRename: true,
      blocks: [expect.objectContaining({
        oldStartLine: 7,
        newStartLine: 7,
        lines: [
          { type: 'context', oldNumber: 7, newNumber: 7, content: ' const fixed = true' },
          { type: 'delete', oldNumber: 8, newNumber: undefined, content: '-const name = "old"' },
          { type: 'insert', oldNumber: undefined, newNumber: 8, content: '+const name = "new"' },
        ],
      })],
    })])

    const rendered = html(presentation.renderer.input, presentation.renderer.options)
    expect(rendered).toContain('d2h-file-side-diff')
    expect(rendered).toContain('function name()')
  })

  it('does not mutate the validated value', () => {
    const value = structuredClone(textDiff)
    const before = structuredClone(value)

    codeDiffPresentation(value)

    expect(value).toEqual(before)
  })

  it('returns the explicit empty state', () => {
    expect(codeDiffPresentation({ schemaVersion: 1, id: 'empty', truncated: false, files: [] }))
      .toMatchObject({ emptyMessage: 'No changes.', truncatedWarning: null, binaryFiles: [], renderer: { input: [] } })
  })

  it('returns binary files as explicit states outside renderer input', () => {
    const presentation = codeDiffPresentation({
      schemaVersion: 1,
      id: 'binary',
      truncated: false,
      files: [{ oldPath: 'asset.png', newPath: 'asset.png', status: 'modified', binary: true, hunks: [] }],
    })

    expect(presentation.binaryFiles).toEqual([{
      oldName: 'asset.png',
      newName: 'asset.png',
      message: 'Binary file changed',
    }])
    expect(presentation.renderer.input).toEqual([])
    expect(presentation.emptyMessage).toBeNull()
  })

  it('returns the truncation warning before complete and empty presentations', () => {
    const presentation = codeDiffPresentation({ schemaVersion: 1, id: 'cut', truncated: true, files: [] })

    expect(presentation.truncatedWarning).toBe('Warning: This diff is truncated.')
    expect(presentation.emptyMessage).toBe('No changes.')
  })
})
