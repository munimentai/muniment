// Northwind Traders, the sample company the first screen offers. It is a
// fixture in this module and nowhere else: the record store never holds it,
// so it cannot appear in the company list and no write can reach it. The
// report it carries is the local report's row shape, computed here against
// numbers rather than against a graph.

export const SAMPLE_COMPANY = 'Northwind Traders'

// What the fixture's graph holds, the one mono line under the report.
const KINDS = [
  { name: 'person', count: 18340 },
  { name: 'organization', count: 6210 },
  { name: 'invoice', count: 12004 },
  { name: 'ticket', count: 9088 },
  { name: 'deal', count: 1455 },
  { name: 'subscription', count: 712 },
]

const CATEGORIES = new Map([
  ['identity', 'Identity'],
  ['freshness', 'Freshness'],
  ['ownership', 'Ownership'],
  ['schema', 'Schema'],
])

// How the open count moved since the run before this one. Negative is fewer.
const MOVED = -3

// Each finding is one sentence: a magnitude, a claim and a consequence. The
// evidence is what Review opens, and every line of it is a record.
const FINDINGS = [
  {
    id: 'duplicate-person',
    category: 'identity',
    magnitude: 4182,
    short: 'duplicate people',
    claim: 'people appear more than once across HubSpot and Salesforce',
    consequence: 'every owner report and every campaign count reads them twice',
    evidence: [
      'person 18,340 rows, 4,182 resolve onto another person',
      'rule lower(email) equal, source ids differ',
      'ada.chen@northwind.example hubspot:contact:4471 salesforce:Contact:0035f00000LmQ1',
      'ada.chen@northwind.example salesforce:Contact:0035f00000LmQ7',
    ],
  },
  {
    id: 'stale-value',
    category: 'freshness',
    magnitude: 2914,
    short: 'stale values',
    claim: 'records hold a value Salesforce changed after HubSpot last read it',
    consequence: 'the older copy answers first',
    evidence: [
      'organization.stage salesforce 2026-09-11, hubspot 2026-06-02',
      'person.title salesforce 2026-09-09, hubspot 2025-11-27',
      'oldest disagreement 471 days',
    ],
  },
  {
    id: 'duplicate-organization',
    category: 'identity',
    magnitude: 1207,
    short: 'duplicate organizations',
    claim: 'organizations carry a name another organization already carries',
    consequence: 'pipeline rolls up to two rows for one account',
    evidence: [
      'organization 6,210 rows, 1,207 share a normalized name',
      'rule name lowercased, legal suffix dropped',
      'Fabrikam Inc salesforce:Account:0015f00000QpX2',
      'Fabrikam salesforce:Account:0015f00000QpZ8',
    ],
  },
  {
    id: 'unreachable-person',
    category: 'freshness',
    magnitude: 848,
    short: 'people no sequence can reach',
    claim: 'people carry no email address and no telephone number',
    consequence: 'a sequence counts them and never reaches them',
    evidence: [
      'person 848 rows with email null and phone null',
      '612 of them sit in an active sequence',
      'oldest 2,104 days since the last activity',
    ],
  },
  {
    id: 'owner-concentration',
    category: 'ownership',
    magnitude: 300,
    short: 'records owned by two people',
    claim: 'records name one of two people as their owner',
    consequence: 'one departure takes a third of the pipeline with it',
    evidence: [
      'deal 1,455 rows, 300 owned by 2 of 41 owners',
      'j.okafor 174 deals, 2.1M open',
      'm.iverson 126 deals, 1.4M open',
    ],
  },
  {
    id: 'domain-collision',
    category: 'identity',
    magnitude: 96,
    short: 'domains claimed twice',
    claim: 'organizations claim a web domain another organization already claims',
    consequence: 'routing sends one account to two owners',
    evidence: [
      'organization 96 rows share a domain with another row',
      'contoso.example salesforce:Account:0015f00000RbK4',
      'contoso.example pipedrive:organization:8821',
    ],
  },
  {
    id: 'dead-field',
    category: 'schema',
    magnitude: 61,
    short: 'fields nobody reads',
    claim: 'fields hold a value on fewer than one record in a thousand',
    consequence: 'a form asks for what no report reads',
    evidence: [
      'person.secondary_fax 3 of 18,340 rows',
      'organization.franchise_code 1 of 6,210 rows',
      '59 more under the same bound',
    ],
  },
  {
    id: 'text-that-is-a-list',
    category: 'schema',
    magnitude: 14,
    short: 'text fields that are lists',
    claim: 'free text fields hold fewer than twelve distinct values',
    consequence: 'a list is doing its work as a paragraph',
    evidence: [
      'deal.loss_reason 7 distinct values over 1,455 rows',
      'ticket.channel 5 distinct values over 9,088 rows',
      '12 more under the same bound',
    ],
  },
]

const count = (value) => value.toLocaleString('en-US')

export function sampleKinds() {
  return KINDS.map((kind) => ({ ...kind }))
}

export function sampleFindings() {
  return FINDINGS.map((finding) => ({ ...finding, evidence: [...finding.evidence] }))
}

// The one mono line the sample's kind holdings read as.
export function sampleKindLine(kinds = sampleKinds()) {
  return kinds.map((kind) => `${kind.name} ${count(kind.count)}`).join(' · ')
}

// One row, one sentence.
export function findingSentence(finding) {
  return `${count(finding.magnitude)} ${finding.claim}, so ${finding.consequence}.`
}

// The same sentence without its magnitude, for a row that sets the number
// beside the claim rather than inside it.
export function findingClaim(finding) {
  return `${finding.claim}, so ${finding.consequence}.`
}

// The largest findings, largest first.
export function topFindings(limit = 3, findings = sampleFindings()) {
  return [...findings].sort((a, b) => b.magnitude - a.magnitude).slice(0, limit)
}

// The proof line the first screen carries: the three largest findings, each
// as its magnitude and its short name, so the claim is the fixture's own
// arithmetic rather than a promise.
export function sampleProofLine(findings = sampleFindings()) {
  return topFindings(3, findings)
    .map((finding) => `${count(finding.magnitude)} ${finding.short}`)
    .join(' · ')
}

// How the open count moved, in the words the report header uses.
export function sampleTrend(moved = MOVED) {
  if (moved === 0) return 'level with the last run'
  const size = Math.abs(moved)
  return `${count(size)} ${moved > 0 ? 'more' : 'fewer'} than the last run`
}

// The report header: the company, the open finding count, and the trend.
export function sampleReportLine(findings = sampleFindings(), moved = MOVED) {
  const open = findings.length
  return `${SAMPLE_COMPANY} · ${count(open)} open ${open === 1 ? 'finding' : 'findings'} · ${sampleTrend(moved)}`
}

// The same line on the first screen, where the company needs naming as the
// sample it is.
export function sampleLeadLine(findings = sampleFindings(), moved = MOVED) {
  return sampleReportLine(findings, moved).replace(SAMPLE_COMPANY, `${SAMPLE_COMPANY}, a sample company`)
}

// Findings grouped by category, groups by their largest finding and rows by
// magnitude, both descending, with the count the group header shows.
export function groupFindings(findings = sampleFindings()) {
  const groups = []
  for (const finding of findings) {
    let group = groups.find((candidate) => candidate.category === finding.category)
    if (!group) {
      group = { category: finding.category, label: CATEGORIES.get(finding.category) ?? finding.category, findings: [] }
      groups.push(group)
    }
    group.findings.push(finding)
  }
  for (const group of groups) {
    group.findings.sort((a, b) => b.magnitude - a.magnitude)
    group.count = group.findings.length
  }
  return groups.sort((a, b) => b.findings[0].magnitude - a.findings[0].magnitude)
}
