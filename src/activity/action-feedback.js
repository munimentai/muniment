const categories = {
  read: { icon: 'book-open', done: 'Read files', active: 'Reading files', verb: 'Read', present: 'Reading' },
  search: { icon: 'search', done: 'Searched files', active: 'Searching files', verb: 'Searched', present: 'Searching' },
  command: { icon: 'square-terminal', done: 'Ran commands', active: 'Running commands', verb: 'Ran', present: 'Running' },
  edit: { icon: 'file-text', done: 'Changed files', active: 'Changing files', verb: 'Changed', present: 'Changing' },
  other: { icon: 'play', done: 'Actions', active: 'Working', verb: '', present: '' },
}

export function actionGroups(activities = [], live = false) {
  const groups = []
  for (const activity of activities) {
    const name = (activity.displayName || 'Action').replace(/[_-]+/g, ' ')
    const kind = /^(read|read file|read files)$/i.test(name) ? 'read'
      : /^(grep|find|ls|search|search files)$/i.test(name) ? 'search'
        : /^(bash|powershell|shell|exec|exec command|run command)$/i.test(name) ? 'command'
          : /^(edit|write|apply patch)$/i.test(name) ? 'edit' : 'other'
    const category = categories[kind]
    let args = {}
    try { args = JSON.parse(activity.input) || {} } catch { /* Older runs have no arguments. */ }
    const running = live && activity.status === 'running'
    const state = activity.status === 'running' ? (live ? 'In progress' : 'Interrupted')
      : activity.status === 'failed' ? 'Failed' : 'Completed'
    const path = args.path || args.file_path || args.filePath
    const target = kind === 'command' ? args.description || args.command
      : kind === 'search' ? args.pattern || args.query || path
        : path && String(path).split(/[\\/]/).pop()
    const verb = running ? category.present : category.verb
    const label = kind === 'command' && args.description ? String(args.description) : target ? `${verb} ${target}`.trim()
      : kind === 'other' ? name.charAt(0).toUpperCase() + name.slice(1) : `${verb} ${kind === 'command' ? 'command' : 'files'}`
    const action = { ...activity, detailInput: actionInput(activity.input), label, state, running, icon: category.icon }
    let group = groups.at(-1)
    if (!group || group.kind !== kind) {
      group = { kind, id: activity.effectId, ...category, actions: [] }
      groups.push(group)
    }
    group.actions.push(action)
    group.running = group.running || running
    group.label = group.running ? group.active : group.done
  }
  return groups
}

export function actionDuration(start, end) {
  const seconds = Math.max(0, Math.floor((new Date(end) - new Date(start)) / 1000))
  if (!Number.isFinite(seconds)) return ''
  return seconds < 60 ? `${seconds}s` : `${Math.floor(seconds / 60)}m ${seconds % 60}s`
}

export function actionInput(input) {
  if (!input) return ''
  try {
    const value = JSON.parse(input)
    if (!value || typeof value !== 'object' || Array.isArray(value)) return input
    return Object.entries(value).map(([key, item]) => {
      const name = key.replace(/[_-]/g, ' ').replace(/([a-z])([A-Z])/g, '$1 $2')
      return `${name.charAt(0).toUpperCase() + name.slice(1)}: ${typeof item === 'string' ? item : JSON.stringify(item, null, 2)}`
    }).join('\n')
  } catch { return input }
}
