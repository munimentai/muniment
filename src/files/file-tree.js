// Expanded folders stay beside their siblings. A filter searches loaded branches.
export function fileTreeRows(entries, children, expanded, filter = '', depth = 0, parent = null, ancestors = new Set()) {
  const query = filter.toLowerCase()
  return entries.flatMap(entry => {
    if (ancestors.has(entry.path)) return []
    const next = new Set(ancestors).add(entry.path)
    const nested = expanded.has(entry.path) ? fileTreeRows(children[entry.path] ?? [],children,expanded,filter,depth+1,entry.path,next) : []
    if (query && !entry.directory && !entry.name.toLowerCase().includes(query)) return []
    return [{...entry,depth,parent},...nested]
  })
}
