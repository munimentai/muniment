// The local report's pure state: the header line, the kind line and the
// grouping the rows share with the sample, computed from the report the
// runtime answers rather than from a fixture.
import { groupFindings } from './record-sample.js'

const count = (value) => Number(value ?? 0).toLocaleString('en-US')

// How the open count moved since the last read, in the header's words. A
// first read has nothing to compare against and says so.
export function reportTrend(moved) {
  if (moved === null || moved === undefined) return 'first read'
  if (moved === 0) return 'level with the last read'
  const size = Math.abs(moved)
  return `${count(size)} ${moved > 0 ? 'more' : 'fewer'} than the last read`
}

// The report header: the company, the open finding count, and the trend.
export function reportLine(companyName, report) {
  const open = report?.open ?? report?.findings?.length ?? 0
  return `${companyName ?? 'This company'} · ${count(open)} open ${open === 1 ? 'finding' : 'findings'} · ${reportTrend(report?.moved)}`
}

// The one mono line the company's kind holdings read as, largest first.
export function reportKindLine(kinds) {
  return (Array.isArray(kinds) ? kinds : [])
    .filter((kind) => kind?.count > 0)
    .map((kind) => `${kind.name} ${count(kind.count)}`)
    .join(' · ')
}

// The findings grouped the way the sample groups them.
export function reportGroups(report) {
  return groupFindings(Array.isArray(report?.findings) ? report.findings : [])
}

// The first sentence of a report failure, one line.
export function reportErrorLine(answer) {
  const message = answer?.error?.message ?? (typeof answer === 'string' ? answer : answer?.message) ?? ''
  const first = String(message).split(/(?<=[.!?])\s/)[0]?.trim() || 'The report did not answer.'
  return first.endsWith('.') ? first : `${first}.`
}
