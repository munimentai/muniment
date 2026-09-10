export function scanRows(report) {
  const assistants = new Map()
  for (const finding of report?.findings ?? []) {
    const row = assistants.get(finding.assistantId) ?? {
      assistantId: finding.assistantId,
      displayName: finding.displayName,
      fileCount: 0,
      capped: false,
      unreadable: false,
    }
    row.fileCount += finding.fileCount
    row.capped ||= finding.capped
    row.unreadable ||= finding.warnings.includes('unreadable')
    assistants.set(finding.assistantId, row)
  }
  return [...assistants.values()]
}

export function scanSummary(rows) {
  const found = rows.filter((row) => row.fileCount > 0)
  if (found.length) return found.map((row) => `${row.displayName}: ${row.fileCount} files`).join(' · ')
  if (rows.some((row) => row.capped || row.unreadable)) return 'Assistant memory scan incomplete'
  return 'No assistant memory found'
}
