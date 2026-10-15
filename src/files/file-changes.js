export const fileName = (path) => String(path).split(/[\\/]/).pop()

// A run's applied diffs and its edit tools reduce to counted entries once per
// array. A streamed text delta replaces the run and keeps both arrays, so the
// counts come from these caches and no input is parsed again. An applied diff
// arrives as its summary: per-file counts without the hunks.
const diffEntryCache = new WeakMap()
const editEntryCache = new WeakMap()

function diffEntries(appliedDiffs) {
  let entries = diffEntryCache.get(appliedDiffs)
  if (entries) return entries
  entries = appliedDiffs.map((applied) => ({
    id: applied.effectId || applied.codeDiffId,
    files: (applied.summary?.files ?? []).flatMap((file) => {
      const path = file.newPath || file.oldPath
      if (!path) return []
      return [{ path, additions: file.additions ?? 0, deletions: file.deletions ?? 0,
        incomplete: file.binary || applied.summary.truncated, deleted: file.status === 'deleted' }]
    }),
  }))
  diffEntryCache.set(appliedDiffs, entries)
  return entries
}

function editEntries(toolActivity) {
  let entries = editEntryCache.get(toolActivity)
  if (entries) return entries
  entries = toolActivity.flatMap((action) => {
    if (action.status !== 'completed' || !/^(edit|write|apply_patch)$/i.test(action.displayName)) return []
    let args
    try { args = JSON.parse(action.input) } catch { return [] }
    const path = args?.path || args?.file_path || args?.filePath
    if (!path) return []
    const oldText = args.oldText ?? args.old_string
    const newText = args.newText ?? args.new_string
    const counted = typeof oldText === 'string' && typeof newText === 'string'
    const lines = (text) => text ? text.replace(/\n$/, '').split('\n').length : 0
    return [{ path, effectId: action.effectId, counted, additions: counted ? lines(newText) : 0, deletions: counted ? lines(oldText) : 0 }]
  })
  editEntryCache.set(toolActivity, entries)
  return entries
}

// Names one applied change by its summary: the file, or the file count, and
// the line counts when every file has them.
export function appliedDiffLabel(summary) {
  const files = summary?.files ?? []
  const noun = (n, one, many) => `${n} ${n === 1 ? one : many}`
  const name = files.length === 1 ? fileName(files[0].newPath || files[0].oldPath) : noun(files.length, 'file', 'files')
  if (summary?.truncated || files.some((file) => file.binary)) return name
  const additions = files.reduce((n, file) => n + (file.additions ?? 0), 0)
  const deletions = files.reduce((n, file) => n + (file.deletions ?? 0), 0)
  return `${name} +${additions} −${deletions}`
}

// Counts describe recorded applied changes, never a proposed or failed edit.
export function changedFiles(messages = []) {
  const files = new Map()
  const effects = new Set()
  for (const message of messages) {
    const run = message.run
    if (!run) continue
    const recordedPaths = new Set()
    for (const { id, files: diffFiles } of run.appliedDiffs ? diffEntries(run.appliedDiffs) : []) {
      if (id && effects.has(id)) continue
      if (id) effects.add(id)
      for (const file of diffFiles) {
        recordedPaths.add(file.path)
        const prior = files.get(file.path)
        files.set(file.path, { path: file.path, name: fileName(file.path), additions: (prior?.additions ?? 0) + file.additions,
          deletions: (prior?.deletions ?? 0) + file.deletions,
          incomplete: !!prior?.incomplete || file.incomplete, deleted: file.deleted })
      }
    }
    for (const edit of run.toolActivity ? editEntries(run.toolActivity) : []) {
      if (recordedPaths.has(edit.path) || (edit.effectId && effects.has(edit.effectId))) continue
      if (edit.effectId) effects.add(edit.effectId)
      const prior = files.get(edit.path)
      files.set(edit.path, { path: edit.path, name: fileName(edit.path), incomplete: !!prior?.incomplete || !edit.counted, additions: (prior?.additions ?? 0) + edit.additions, deletions: (prior?.deletions ?? 0) + edit.deletions })
    }
  }
  return [...files.values()]
}

// The thread's changed files, kept as the same array while no run's applied
// diffs or tool activity change, so streamed text does not recount them.
export function createChangedFiles() {
  let sources = []
  let result = []
  return (messages = []) => {
    const next = messages.flatMap(({ run }) => run ? [run.appliedDiffs, run.toolActivity] : [])
    if (next.length === sources.length && next.every((source, index) => source === sources[index])) return result
    sources = next
    result = changedFiles(messages)
    return result
  }
}
