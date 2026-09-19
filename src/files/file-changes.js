export const fileName = (path) => String(path).split(/[\\/]/).pop()

// Counts describe recorded applied changes, never a proposed or failed edit.
export function changedFiles(messages = []) {
  const files = new Map()
  const effects = new Set()
  for (const message of messages) {
    const run = message.run
    if (!run) continue
    const recordedPaths = new Set()
    for (const applied of run.appliedDiffs ?? []) {
      const id = applied.effectId || applied.codeDiffId || applied.diff?.id
      if (id && effects.has(id)) continue
      if (id) effects.add(id)
      for (const file of applied.diff?.files ?? []) {
        const path = file.newPath || file.oldPath
        if (!path) continue
        recordedPaths.add(path)
        const prior = files.get(path)
        const lines = (file.hunks ?? []).flatMap((hunk) => hunk.lines)
        files.set(path, { path, name: fileName(path), additions: (prior?.additions ?? 0) + lines.filter((l) => l.kind === 'addition').length,
          deletions: (prior?.deletions ?? 0) + lines.filter((l) => l.kind === 'deletion').length,
          incomplete: !!prior?.incomplete || file.binary || applied.diff.truncated, deleted: file.status === 'deleted' })
      }
    }
    for (const action of run.toolActivity ?? []) {
      if (action.status !== 'completed' || !/^(edit|write|apply_patch)$/i.test(action.displayName)) continue
      let args
      try { args = JSON.parse(action.input) } catch { continue }
      const path = args?.path || args?.file_path || args?.filePath
      if (!path || recordedPaths.has(path) || (action.effectId && effects.has(action.effectId))) continue
      if (action.effectId) effects.add(action.effectId)
      const prior = files.get(path)
      const oldText = args.oldText ?? args.old_string
      const newText = args.newText ?? args.new_string
      const counted = typeof oldText === 'string' && typeof newText === 'string'
      const lines = (text) => text ? text.replace(/\n$/, '').split('\n').length : 0
      files.set(path, { path, name: fileName(path), incomplete: !!prior?.incomplete || !counted, additions: (prior?.additions ?? 0) + (counted ? lines(newText) : 0), deletions: (prior?.deletions ?? 0) + (counted ? lines(oldText) : 0) })
    }
  }
  return [...files.values()]
}
