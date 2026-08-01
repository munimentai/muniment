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
      matching: 'none',
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
          { type: 'context', oldNumber: 7, newNumber: 7, content: ' const fixed = true', segments: [] },
          { type: 'delete', oldNumber: 8, newNumber: undefined, content: '-const name = "old"', segments: [] },
          { type: 'insert', oldNumber: undefined, newNumber: 8, content: '+const name = "new"', segments: [] },
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
      position: 0,
      message: 'Binary file changed',
    }])
    expect(presentation.renderer.input).toEqual([])
    expect(presentation.emptyMessage).toBeNull()
  })

  it('preserves contract order across text and binary presentation states', () => {
    const secondTextFile = structuredClone(textDiff.files[0])
    secondTextFile.oldPath = 'src/second.js'
    secondTextFile.newPath = 'src/second.js'
    const presentation = codeDiffPresentation({
      schemaVersion: 1,
      id: 'mixed',
      truncated: false,
      files: [
        textDiff.files[0],
        { oldPath: 'asset.png', newPath: 'asset.png', status: 'modified', binary: true, hunks: [] },
        secondTextFile,
      ],
    })

    expect(presentation.renderer.input.map(({ newName, position }) => ({ newName, position }))).toEqual([
      { newName: 'src/new.js', position: 0 },
      { newName: 'src/second.js', position: 2 },
    ])
    expect(presentation.binaryFiles).toEqual([expect.objectContaining({ newName: 'asset.png', position: 1 })])
  })

  it('preserves producer-supplied segment boundaries', () => {
    const value = structuredClone(textDiff)
    value.files[0].hunks[0].lines[2].segments = [
      { kind: 'plain', text: 'const name = "' },
      { kind: 'addition', text: 'new' },
      { kind: 'plain', text: '"' },
    ]

    const presentation = codeDiffPresentation(value)

    expect(presentation.renderer.options.matching).toBe('none')
    expect(presentation.renderer.input[0].blocks[0].lines[2].segments).toEqual([
      { kind: 'plain', text: 'const name = "' },
      { kind: 'addition', text: 'new' },
      { kind: 'plain', text: '"' },
    ])
  })

  it('returns the truncation warning before complete and empty presentations', () => {
    const presentation = codeDiffPresentation({ schemaVersion: 1, id: 'cut', truncated: true, files: [] })

    expect(presentation.truncatedWarning).toBe('Warning: This diff is truncated.')
    expect(presentation.emptyMessage).toBe('No changes.')
  })
})
