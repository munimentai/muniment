// Copy holds the core write plan's caps: files per import, bytes per import
// and bytes per file.
export const COPY_LIMITS = { files: 128, bytes: 256 * 1024, document: 64 * 1024 }

export function scanRows(report) {
  const assistants = new Map()
  for (const finding of report?.findings ?? []) {
    const row = assistants.get(finding.assistantId) ?? {
      assistantId: finding.assistantId,
      displayName: finding.displayName,
      fileCount: 0,
      byteTotal: 0,
      capped: false,
      unreadable: false,
    }
    row.fileCount += finding.fileCount
    row.byteTotal += finding.byteTotal ?? 0
    row.capped ||= finding.capped
    row.unreadable ||= finding.warnings.includes('unreadable')
    assistants.set(finding.assistantId, row)
  }
  return [...assistants.values()]
}

export function copyFits(row) {
  return row.fileCount <= COPY_LIMITS.files && row.byteTotal <= COPY_LIMITS.bytes
}

export function scanSummary(rows) {
  const found = rows.filter((row) => row.fileCount > 0)
  if (found.length) return found.map((row) => `${row.displayName}: ${row.fileCount} files`).join(' · ')
  if (rows.some((row) => row.capped || row.unreadable)) return 'Assistant memory scan incomplete'
  return 'No assistant memory found'
}
