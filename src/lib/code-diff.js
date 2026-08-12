const lineTypes = {
  context: 'context',
  addition: 'insert',
  deletion: 'delete',
}

export const codeDiffRendererOptions = Object.freeze({
  outputFormat: 'side-by-side',
  drawFileList: false,
  rawTemplates: {
    'generic-file-path': '<span class="d2h-file-name-wrapper">{{>fileIcon}}<span class="d2h-file-name">{{fileDiffName}}</span> {{>fileTag}}</span>',
    'tag-file-added': '<span class="d2h-tag d2h-added d2h-added-tag">added</span>',
    'tag-file-changed': '<span class="d2h-tag d2h-changed d2h-changed-tag">changed</span>',
    'tag-file-deleted': '<span class="d2h-tag d2h-deleted d2h-deleted-tag">deleted</span>',
    'tag-file-renamed': '<span class="d2h-tag d2h-moved d2h-moved-tag">renamed</span>',
  },
  matching: 'none',
  matchingMaxComparisons: 2500,
  maxLineSizeInBlockForComparison: 200,
  maxLineLengthHighlight: 10000,
})

function rendererLine(line) {
  return {
    type: lineTypes[line.kind],
    oldNumber: line.oldLineNumber,
    newNumber: line.newLineNumber,
    content: `${line.kind === 'addition' ? '+' : line.kind === 'deletion' ? '-' : ' '}${line.text}`,
    segments: line.segments,
  }
}

function rendererFile(file, position) {
  const lines = file.hunks.flatMap((hunk) => hunk.lines)

  return {
    oldName: file.oldPath ?? '/dev/null',
    newName: file.newPath ?? '/dev/null',
    position,
    oldMode: file.oldMode,
    newMode: file.newMode,
    addedLines: lines.filter((line) => line.kind === 'addition').length,
    deletedLines: lines.filter((line) => line.kind === 'deletion').length,
    isCombined: false,
    isGitDiff: true,
    isNew: file.status === 'added',
    isDeleted: file.status === 'deleted',
    isRename: file.status === 'renamed',
    language: '',
    blocks: file.hunks.map((hunk) => ({
      oldStartLine: hunk.oldStart,
      newStartLine: hunk.newStart,
      header: hunk.header,
      lines: hunk.lines.map(rendererLine),
    })),
  }
}

function binaryFile(file, position) {
  return {
    oldName: file.oldPath ?? '/dev/null',
    newName: file.newPath ?? '/dev/null',
    position,
    message: 'Binary file changed',
  }
}

export function codeDiffPresentation(codeDiff) {
  const binaryFiles = []
  const textFiles = []

  codeDiff.files.forEach((file, position) => {
    if (file.binary) {
      binaryFiles.push(binaryFile(file, position))
    } else {
      textFiles.push(rendererFile(file, position))
    }
  })

  return {
    id: codeDiff.id,
    emptyMessage: codeDiff.files.length === 0 ? 'No changes.' : null,
    truncatedWarning: codeDiff.truncated ? 'Warning: This diff is truncated.' : null,
    binaryFiles,
    renderer: {
      input: textFiles,
      options: codeDiffRendererOptions,
    },
  }
}
