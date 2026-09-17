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

// The identity kinds a column fills on a kind that has no property of that
// name. The runtime lands the column as an identity beside the key, so a
// person keyed on a source id keeps the email the row carries.
const IDENTITY_TARGETS = { person: ['email', 'phone', 'handle'] }

// Every property of the kind that an import can fill, core first, then own,
// then the identities the kind takes beside its properties.
export function targetProperties(kind) {
  const core = Object.keys(kind?.schema?.properties ?? {})
  const own = Object.keys(kind?.extension?.properties ?? {})
  const identities = (IDENTITY_TARGETS[kind?.name] ?? []).filter((name) => !core.includes(name) && !own.includes(name))
  return [...core, ...own, ...identities]
}

// How the form names a target: an own property reads `(own)`, an identity
// beside the key reads `(identity)`.
export function propertyLabel(kind, property) {
  if (property.startsWith('x_')) return `${property.slice(2)} (own)`
  if (!(property in (kind?.schema?.properties ?? {})) && (IDENTITY_TARGETS[kind?.name] ?? []).includes(property)) return `${property} (identity)`
  return property
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

// The sources an import reads: a CSV file, then every network source the
// sidecar knows, each naming the credentials it connects with. A source with
// one credential sends it as is. A source with several sends them packed as
// one JSON object keyed by name, which the sidecar reads back.
export function sourceOptions() {
  const token = (label) => [{ name: 'token', label, secret: true }]
  return [
    { value: 'csv', label: 'CSV file', note: 'a file on this machine' },
    { value: 'stripe', label: 'Stripe', note: 'customers, subscriptions, invoices', secret: 'secret key', credentials: token('Secret key') },
    { value: 'hubspot', label: 'HubSpot', note: 'contacts, companies, deals, tickets', secret: 'private app access token', credentials: token('Private app access token') },
    { value: 'pipedrive', label: 'Pipedrive', note: 'people, organizations, deals', secret: 'API token', credentials: token('API token') },
    { value: 'salesforce', label: 'Salesforce', note: 'accounts, contacts, leads, opportunities, cases', secret: 'connected app', credentials: [
      { name: 'instance_url', label: 'My Domain URL, https://acme.my.salesforce.com', secret: false },
      { name: 'client_id', label: 'Consumer key', secret: true },
      { name: 'client_secret', label: 'Consumer secret', secret: true },
    ] },
    { value: 'zendesk', label: 'Zendesk', note: 'tickets, users, organizations', secret: 'API token', credentials: [
      { name: 'subdomain', label: 'Subdomain, the acme in acme.zendesk.com', secret: false },
      { name: 'email', label: 'Agent email', secret: false },
      { name: 'api_token', label: 'API token', secret: true },
    ] },
    { value: 'intercom', label: 'Intercom', note: 'contacts, companies, conversations', secret: 'access token', credentials: token('Access token') },
    { value: 'freshdesk', label: 'Freshdesk', note: 'tickets, contacts, companies', secret: 'API key', credentials: [
      { name: 'domain', label: 'Domain, the acme in acme.freshdesk.com', secret: false },
      { name: 'api_key', label: 'API key', secret: true },
    ] },
  ]
}

// The credentials a source asks for, one secret key when it names none.
export function sourceCredentials(source) {
  return sourceOptions().find((option) => option.value === source)?.credentials ?? [{ name: 'token', label: 'Secret key', secret: true }]
}

// Whether every credential a source asks for has a value.
export function credentialsFilled(source, values) {
  return sourceCredentials(source).every((credential) => String(values?.[credential.name] ?? '').trim() !== '')
}

// The one secret the runtime stores: the value itself for one credential,
// a JSON object keyed by name for several.
export function packSecret(source, values) {
  const credentials = sourceCredentials(source)
  if (credentials.length === 1) return String(values?.[credentials[0].name] ?? '').trim()
  return JSON.stringify(Object.fromEntries(credentials.map((credential) => [credential.name, String(values?.[credential.name] ?? '').trim()])))
}

// The external id prefix an object's rows key on: `csv:<file stem>` for a
// file, `<source>:<object>` for a network source, the record's
// `system:object` shape either way.
export function externalPrefix(description) {
  const source = description?.source ?? 'csv'
  if (source === 'csv') {
    const stem = String(description?.label ?? 'file').replace(/\.[^.]+$/, '').replace(/[^A-Za-z0-9]+/g, '_').toLowerCase() || 'file'
    return `csv:${stem}`
  }
  return `${source}:${String(description?.object ?? 'object').replace(/[^A-Za-z0-9]+/g, '_').toLowerCase()}`
}

// The identity choices: the title's name key, then every field the samples
// type as an email, a domain or a phone, then any field as an external id
// keyed to this object. A field the source types as an id leads the list.
export function identityOptions(description) {
  const prefix = externalPrefix(description)
  const options = [{ value: '', label: 'the title' }]
  for (const field of description?.fields ?? []) {
    if (field.guess === 'id') options.push({ value: `external:${prefix}:${field.name}`, label: `id in ${field.name}` })
  }
  for (const field of description?.fields ?? []) {
    if (['email', 'domain', 'phone'].includes(field.guess)) options.push({ value: `${field.guess}:${field.name}`, label: `${field.guess} in ${field.name}` })
  }
  for (const field of description?.fields ?? []) {
    if (field.guess !== 'id') options.push({ value: `external:${prefix}:${field.name}`, label: `id in ${field.name}` })
  }
  return options
}

// The first identity option a description suggests: the source's own id,
// else an email, else a domain, else a phone, else the title.
export function suggestIdentity(description) {
  const options = identityOptions(description)
  const own = (description?.fields ?? []).find((field) => field.guess === 'id')
  if (own) return options.find((candidate) => candidate.value.endsWith(`:${own.name}`) && candidate.value.startsWith('external:'))?.value ?? ''
  for (const kind of ['email', 'domain', 'phone']) {
    const option = options.find((candidate) => candidate.value.startsWith(`${kind}:`))
    if (option) return option.value
  }
  return ''
}

// The objects a column that holds another record's id points at, by the
// column's folded name, and the kind those objects land on.
const EDGE_TARGETS = {
  customer: { objects: ['customers'], kind: 'org' },
  customerid: { objects: ['customers'], kind: 'org' },
  company: { objects: ['companies'], kind: 'org' },
  companyid: { objects: ['companies'], kind: 'org' },
  organization: { objects: ['organizations', 'organisations'], kind: 'org' },
  organizationid: { objects: ['organizations', 'organisations'], kind: 'org' },
  orgid: { objects: ['organizations', 'organisations'], kind: 'org' },
  account: { objects: ['accounts'], kind: 'org' },
  accountid: { objects: ['accounts'], kind: 'org' },
  contact: { objects: ['contacts'], kind: 'person' },
  contactid: { objects: ['contacts'], kind: 'person' },
  person: { objects: ['persons', 'people'], kind: 'person' },
  personid: { objects: ['persons', 'people'], kind: 'person' },
}

// The edges a network object's rows draw: each id column that names another
// object of the same source, through the first relation the catalogue
// allows from this kind to that object's kind. A subscription's `customer`
// draws `billed_to` through `external:stripe:customers:customer`. A CSV
// names no sibling object, so it draws nothing.
export function suggestEdges(description, kind, relations) {
  const source = description?.source ?? 'csv'
  if (source === 'csv' || !kind?.name || !Array.isArray(relations)) return []
  const edges = []
  for (const field of description?.fields ?? []) {
    if (field.guess !== 'id' || foldName(field.name) === 'id') continue
    const target = EDGE_TARGETS[foldName(field.name)]
    if (!target || target.kind === kind.name) continue
    const relation = relations.find((candidate) => (candidate.from.includes(kind.name) || candidate.from.includes('any')) && candidate.to.includes(target.kind))
    if (!relation) continue
    edges.push({ relation: relation.name, identity: `external:${source}:${target.objects[0]}:${field.name}` })
  }
  return edges
}

// The mapping record the form proposes. Only mapped fields are kept.
export function mappingData(description, kind, fields, identity, edges = []) {
  const mapped = Object.fromEntries(Object.entries(fields ?? {}).filter(([, property]) => property))
  const data = { source: description.source ?? 'csv', object: description.object, kind: kind.name, fields: mapped, approved: true }
  if (identity) data.identity = identity
  if (edges.length) data.edges = edges
  return data
}

// The proposed mapping as one mono line per column, then the key, each
// edge, the kind and the file, so the diff reads as what the run will do.
export function mappingLines(description, kind, fields, identity, edges = []) {
  const lines = Object.entries(fields ?? {}).filter(([, property]) => property).map(([column, property]) => `${column} fills ${propertyLabel(kind, property)}`)
  lines.push(`keyed on ${identity ? identity.replace(/^([a-z_]+):(.*)$/, (_, kindName, rest) => `${kindName} in ${rest.split(':').pop()}`) : 'the title'}`)
  for (const edge of edges) {
    const parts = edge.identity.split(':')
    lines.push(`${parts[parts.length - 1]} links ${edge.relation} to ${parts.length > 3 ? parts[2] : parts[0]}`)
  }
  lines.push(`kind ${kind?.name ?? ''}`.trim())
  lines.push(`${description?.source && description.source !== 'csv' ? description.source : 'file'} ${description?.label ?? description?.object ?? ''}`.trim())
  return lines
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
  lines.push(`${run.created ?? 0} created, ${run.updated ?? 0} updated, ${run.unchanged ?? 0} unchanged${run.linked ? `, ${run.linked} linked` : ''}`)
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
    linked: sum('linked'),
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

// The kind a source object fills by default, from the object's name. A CSV
// file names no kind, and the user picks one.
export function suggestKind(source, object) {
  const name = String(object ?? '').toLowerCase()
  if (source === 'csv') return null
  if (/contact|person|people|user|lead/.test(name)) return 'person'
  if (/compan|organi|account|customer/.test(name)) return 'org'
  if (/deal|opportunit/.test(name)) return 'deal'
  if (/ticket|case|conversation/.test(name)) return 'ticket'
  if (/invoice/.test(name)) return 'invoice'
  if (/subscription/.test(name)) return 'subscription'
  return null
}
