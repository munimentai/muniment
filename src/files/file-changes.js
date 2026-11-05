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
    // Pi's edit carries its replacements in `edits`, other tools one pair.
    const pairs = Array.isArray(args.edits) ? args.edits : [args]
    const texts = pairs.map((pair) => [pair?.oldText ?? pair?.old_string, pair?.newText ?? pair?.new_string])
    const counted = texts.length > 0 && texts.every(([oldText, newText]) => typeof oldText === 'string' && typeof newText === 'string')
    const lines = (text) => text ? text.replace(/\n$/, '').split('\n').length : 0
    const sum = (side) => counted ? texts.reduce((n, pair) => n + lines(pair[side]), 0) : 0
    return [{ path, effectId: action.effectId, counted, additions: sum(1), deletions: sum(0) }]
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

// A thread's own folder under Home's `sessions` keeps its id suffix, the last
// eight letters and digits of the thread id, when it takes the thread's
// title after a turn. So that suffix and the parent name one folder across
// its names. Any other folder is its own key.
export function folderKey(folder) {
  const trimmed = String(folder).replace(/[\\/]+$/, '')
  const parts = trimmed.split(/[\\/]/)
  const base = parts.pop()
  if (parts.at(-1) !== 'sessions') return trimmed
  return `${parts.join('/')}/#${base.replace(/[^A-Za-z0-9]/g, '').slice(-8)}`
}

// A turn's files as core's checkpoints saved them: one entry per path in the
// turn's folder, with line counts from the stored diff. Core names each path
// relative to the folder. A turn answer read before the thread folder took
// its title names the old folder, so `folders` maps each folder key to the
// newest folder the turns name, and every turn names one file the same way.
// A turn whose records list no file keeps its tool calls, which may name a
// file outside the folder.
function savedEntries(saved, folders) {
  if (!saved?.available || saved.more || !saved.folder || !saved.files?.length) return null
  const named = folders?.get(folderKey(saved.folder)) ?? saved.folder
  const separator = named.includes('\\') && !named.includes('/') ? '\\' : '/'
  const folder = named.replace(/[\\/]+$/, '')
  return saved.files.map((file) => ({
    path: `${folder}${separator}${file.path.split('/').join(separator)}`,
    additions: file.additions ?? 0,
    deletions: file.deletions ?? 0,
    incomplete: file.binary || file.additions == null || file.deletions == null,
    deleted: file.status === 'deleted',
  }))
}

// Counts describe recorded applied changes, never a proposed or failed edit.
// `saved` maps a run to core's `run.changes` answer, which stands for that
// turn in place of its tool calls.
export function changedFiles(messages = [], saved = {}) {
  const files = new Map()
  const effects = new Set()
  const folders = new Map()
  for (const { run } of messages) {
    const folder = run && saved?.[run.id]?.available && saved[run.id].folder
    if (folder) folders.set(folderKey(folder), folder)
  }
  for (const message of messages) {
    const run = message.run
    if (!run) continue
    const turn = savedEntries(saved?.[run.id], folders)
    if (turn) {
      for (const file of turn) {
        const prior = files.get(file.path)
        files.set(file.path, { path: file.path, name: fileName(file.path), additions: (prior?.additions ?? 0) + file.additions,
          deletions: (prior?.deletions ?? 0) + file.deletions, incomplete: !!prior?.incomplete || file.incomplete, deleted: file.deleted })
      }
      continue
    }
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
const NO_SAVED_TURNS = {}
export function createChangedFiles() {
  let sources = []
  let result = []
  return (messages = [], saved = NO_SAVED_TURNS) => {
    const next = [saved, ...messages.flatMap(({ run }) => run ? [run.appliedDiffs, run.toolActivity] : [])]
    if (next.length === sources.length && next.every((source, index) => source === sources[index])) return result
    sources = next
    result = changedFiles(messages, saved)
    return result
  }
}

// A command can write any file, so only the turn's checkpoint knows what a
// turn that ran one changed.
export function ranCommands(run) {
  return (run?.toolActivity ?? []).some((action) => action.presentation?.kind === 'command' || /^(bash|powershell)$/i.test(action.displayName ?? ''))
}
