// The import's pure state: which kind property a source field suggests, the
// identity choices a description offers, the mapping record the form
// proposes, and the lines a run summary reads as.

// Words a source calls a property. The key is the kind property, the values
// are folded field names that fill it.
const SYNONYMS = {
  name: ['company', 'companyname', 'organization', 'organisation', 'org', 'account', 'accountname', 'customer', 'customername', 'vendor', 'business', 'legalname', 'title', 'deal', 'dealname', 'subject', 'summary', 'project', 'task', 'service'],
  legal_name: ['legalname', 'registeredname'],
  domain: ['website', 'web', 'url', 'site', 'homepage', 'domainname'],
  full_name: ['name', 'contact', 'contactname', 'person', 'fullname', 'customer', 'customername', 'owner', 'assignee'],
  given_name: ['first', 'firstname', 'givenname', 'forename'],
  family_name: ['last', 'lastname', 'familyname', 'surname'],
  email: ['email', 'emailaddress', 'mail', 'primaryemail', 'workemail'],
  phone: ['phone', 'phonenumber', 'tel', 'telephone', 'mobile', 'cell'],
  job_title: ['title', 'jobtitle', 'role', 'position'],
  industry: ['industry', 'sector', 'vertical'],
  city: ['city', 'town'],
  state: ['state', 'region', 'province'],
  country: ['country'],
  amount: ['amount', 'value', 'total', 'price', 'arr', 'mrr', 'revenue', 'dealvalue'],
  currency: ['currency', 'ccy'],
  stage: ['stage', 'dealstage', 'pipelinestage'],
  status: ['status', 'state'],
  priority: ['priority', 'severity', 'urgency'],
  expected_close: ['closedate', 'expectedclose', 'closeddate', 'expectedclosedate'],
  due_at: ['due', 'duedate', 'deadline'],
  description: ['description', 'notes', 'note', 'details', 'body'],
  employee_band: ['employees', 'headcount', 'size', 'companysize'],
  number: ['number', 'id', 'ticketnumber', 'invoicenumber', 'no'],
  plan: ['plan', 'tier', 'product'],
  started_at: ['start', 'startdate', 'started', 'since', 'created', 'createdat', 'signupdate'],
  ends_at: ['end', 'enddate', 'renewal', 'renewaldate', 'expires'],
  issued_at: ['issued', 'issuedate', 'date', 'invoicedate'],
  paid_at: ['paid', 'paiddate'],
  starts_at: ['start', 'starttime', 'begins'],
}

export function foldName(text) {
  return String(text ?? '').toLowerCase().replace(/[^a-z0-9]/g, '')
}

// Every property of the kind that an import can fill, core first, then own.
export function targetProperties(kind) {
  const core = Object.keys(kind?.schema?.properties ?? {})
  const own = Object.keys(kind?.extension?.properties ?? {})
  return [...core, ...own]
}

// The property a field most likely fills: an exact name, then a synonym,
// then the field's guessed type when the kind has one property of that type.
export function suggestProperty(field, kind, taken = new Set()) {
  const properties = targetProperties(kind).filter((property) => !taken.has(property))
  if (properties.length === 0) return ''
  const folded = foldName(field?.name)
  const byName = properties.find((property) => foldName(property) === folded || foldName(property.replace(/^x_/, '')) === folded)
  if (byName) return byName
  for (const property of properties) {
    const words = SYNONYMS[property.replace(/^x_/, '')] ?? []
    if (words.includes(folded)) return property
  }
  const guess = field?.guess
  if (guess === 'email' && properties.includes('email')) return 'email'
  if (guess === 'domain' && properties.includes('domain')) return 'domain'
  if (guess === 'phone' && properties.includes('phone')) return 'phone'
  return ''
}

// One suggestion per field, no property twice.
export function suggestFields(description, kind) {
  const taken = new Set()
  const fields = {}
  for (const field of description?.fields ?? []) {
    const property = suggestProperty(field, kind, taken)
    if (property) taken.add(property)
    fields[field.name] = property
  }
  return fields
}

// The identity choices: the title's name key, then every field the samples
// type as an email, a domain or a phone, then any field as an external id
// keyed to this file.
export function identityOptions(description) {
  const stem = String(description?.label ?? 'file').replace(/\.[^.]+$/, '').replace(/[^A-Za-z0-9]+/g, '_').toLowerCase() || 'file'
  const options = [{ value: '', label: 'the title' }]
  for (const field of description?.fields ?? []) {
    if (['email', 'domain', 'phone'].includes(field.guess)) options.push({ value: `${field.guess}:${field.name}`, label: `${field.guess} in ${field.name}` })
  }
  for (const field of description?.fields ?? []) {
    options.push({ value: `external:csv:${stem}:${field.name}`, label: `id in ${field.name}` })
  }
  return options
}

// The first identity option a description suggests: an email, else a domain,
// else a phone, else the title.
export function suggestIdentity(description) {
  const options = identityOptions(description)
  for (const kind of ['email', 'domain', 'phone']) {
    const option = options.find((candidate) => candidate.value.startsWith(`${kind}:`))
    if (option) return option.value
  }
  return ''
}

// The mapping record the form proposes. Only mapped fields are kept.
export function mappingData(description, kind, fields, identity) {
  const mapped = Object.fromEntries(Object.entries(fields ?? {}).filter(([, property]) => property))
  const data = { source: description.source ?? 'csv', object: description.object, kind: kind.name, fields: mapped, approved: true }
  if (identity) data.identity = identity
  return data
}

export function mappedCount(fields) {
  return Object.values(fields ?? {}).filter(Boolean).length
}

// One mono line per count the run reports, and the queue below it.
export function runSummaryLines(run) {
  if (!run) return []
  const lines = []
  const plural = (n, word) => `${n} ${word}${n === 1 ? '' : 's'}`
  lines.push(`${plural(run.total ?? 0, 'row')} in ${run.label || run.object || 'the file'}`)
  lines.push(`${run.created ?? 0} created, ${run.updated ?? 0} updated, ${run.unchanged ?? 0} unchanged`)
  if (run.unplaced) lines.push(`${plural(run.unplaced, 'row')} not placed`)
  if (run.changed === false) lines.push('The file is the one the last run read.')
  return lines
}

// A tally across the calls one run takes.
export function accumulateRun(total, run) {
  const sum = (key) => (total?.[key] ?? 0) + (run?.[key] ?? 0)
  return {
    ...run,
    created: sum('created'),
    updated: sum('updated'),
    unchanged: sum('unchanged'),
    unplaced: sum('unplaced'),
    queue: [...(total?.queue ?? []), ...(run?.queue ?? [])].slice(0, 100),
  }
}

// The first sentence of an import failure, with the runtime's code dropped.
export function importErrorLine(answer) {
  const message = answer?.error?.message ?? (typeof answer === 'string' ? answer : answer?.message) ?? ''
  const first = String(message).split(/(?<=[.!?])\s/)[0]?.trim() || 'The import did not answer.'
  return first.endsWith('.') ? first : `${first}.`
}
