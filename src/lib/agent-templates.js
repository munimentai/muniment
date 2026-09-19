import { avatarFor, avatarSvg, newAvatar } from './agent-avatar.js'
const MAX_BYTES = 131072
const sections = ['skills', 'routines', 'plugins', 'memories', 'settings']
function bounded(content) {
  if (new TextEncoder().encode(content).length > MAX_BYTES) throw new Error('Choose a template under 128 KB.')
}
export function exportTemplate(agent) {
  const file = JSON.stringify({ format: 'muniment-agent', version: 2, name: agent.name, label: agent.label || '', description: agent.instructions,
    avatar: avatarFor(agent), schedule: agent.schedule ? { ...agent.schedule, enabled: false } : null,
    ...(agent.template ? { template: agent.template } : {}) }, null, 2) + '\n'
  bounded(file)
  return file
}
export function exportGrokSetup(agent) {
  const file = `# Set up ${agent.name} in Grok Bot\n\nCreate a new Bot from the configuration below. Set its name, label, and description. Treat the description as lasting instructions. Preserve the supplied skills and reference memories. Review and configure required plugins with the user. Keep routines paused until the user enables them. Do not copy account credentials, conversation history, or private memory.\n\nThe source template may contain only a public profile. Do not claim that missing skills or integrations have been transferred.\n\n## Configuration\n\n\`\`\`json\n${exportTemplate(agent)}\`\`\`\n\n## Avatar\n\nThis original SVG can be used as the Bot avatar where image upload is available.\n\n\`\`\`svg\n${avatarSvg(avatarFor(agent))}\n\`\`\`\n`
  bounded(file)
  return file
}
export function importTemplate(content) {
  bounded(content)
  const text = content.trim()
  let source
  const config = text.match(/```json\s*\n([\s\S]*?)\n```/)
  if (text.startsWith('{') || config) source = JSON.parse(config ? config[1] : text)
  else {
    const match = text.match(/^#\s+(.+)\r?\n/)
    if (!match) throw new Error('Choose a JSON template or Markdown with a name heading and description.')
    source = { name: match[1].trim(), description: text.slice(match[0].length).trim() }
  }
  if (!source || typeof source !== 'object' || Array.isArray(source)) throw new Error('The template must contain an agent profile.')
  if (source.format === 'muniment-agent' && ![1, 2].includes(source.version)) throw new Error('This template version is not supported.')
  const raw = source
  if (!source.name && !source.botName) source = source.bot ?? source.template ?? source
  const name = source.name ?? source.botName
  const instructions = source.instructions || source.description
  const label = source.label ?? source.jobTitle ?? ''
  if (typeof name !== 'string' || !name.trim() || name.length > 100 || typeof instructions !== 'string' || !instructions.trim()) throw new Error('The template needs a name and description.')
  if (typeof label !== 'string' || new TextEncoder().encode(label).length > 120) throw new Error('Keep the job title under 120 bytes.')
  if (new TextEncoder().encode(instructions).length > 65536) throw new Error('The description exceeds 64 KB.')
  const s = source.schedule
  let schedule = null
  if (s && ['daily', 'weekdays', 'weekly'].includes(s.cadence) && /^([01]\d|2[0-3]):[0-5]\d$/.test(s.time) && Number.isInteger(s.weekday) && s.weekday >= 0 && s.weekday <= 6) schedule = { enabled: false, cadence: s.cadence, time: s.time, weekday: s.weekday }
  else if (s && source.format === 'muniment-agent') throw new Error('The template schedule is invalid.')
  const avatar = ['muniment-v1', 'muniment-v2'].includes(source.avatar?.style) && typeof source.avatar.seed === 'string' && source.avatar.seed.length > 0 && source.avatar.seed.length <= 128 ? { ...source.avatar } : newAvatar()
  let template = source.format === 'muniment-agent' ? source.template ?? null : { source: 'imported', coverage: 'provided', original: raw }
  if (template != null && (typeof template !== 'object' || Array.isArray(template))) throw new Error('The template context must be an object.')
  if (template && source.format !== 'muniment-agent') for (const key of sections) if (source[key] != null) template[key] = source[key]
  return { id: '', name: name.trim(), label: label.trim(), instructions, avatar, template, projectId: null, schedule }
}
export function publicTemplateUrl(input) {
  let url
  try { url = new URL(input.trim()) } catch { throw new Error('Paste a public x.ai/bot share link.') }
  const parts = url.pathname.split('/').filter(Boolean)
  if (url.protocol !== 'https:' || url.hostname !== 'x.ai' || url.username || url.password || url.port || parts[0] !== 'bot' || parts.length < 2 || parts.length > 3 || !/^[A-Za-z0-9_-]{21}$/.test(parts[1])) throw new Error('Paste a public x.ai/bot share link.')
  return `https://x.ai/bot/${parts[1]}`
}
export function importGrokPage({ url: input, html }) {
  const url = publicTemplateUrl(input)
  const id = url.split('/').pop()
  // Parse inert serialized page data. Never run the page scripts or insert its HTML.
  let found
  function walk(value) {
    if (!value || typeof value !== 'object') return
    if (value.id === id && typeof value.botName === 'string' && typeof value.description === 'string') found = value
    for (const item of Object.values(value)) walk(item)
  }
  const chunks = []
  for (const script of html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/gi)) {
    for (const match of script[1].matchAll(/self\.__next_f\.push\((\[[\s\S]*?\])\)/g)) {
      try { const row = JSON.parse(match[1]); if (typeof row[1] === 'string') chunks.push(row[1]) } catch { /* Not a serialized data chunk. */ }
    }
  }
  for (const line of chunks.join('').split('\n')) {
    try { walk(JSON.parse(line.slice(line.indexOf(':') + 1))) } catch { /* Module records are not JSON data. */ }
  }
  if (!found) throw new Error('The public profile could not be read. The template may be private, deleted, or use a new format.')
  const draft = importTemplate(JSON.stringify(found))
  draft.template = { ...draft.template, source: 'grok', sourceUrl: url, coverage: 'public-profile' }
  return draft
}
export function templateSummary(template) {
  if (!template) return ''
  const included = sections.filter(key => template[key] != null)
  return [template.coverage === 'public-profile' ? 'Public Grok profile imported. The share page does not provide the full Bot package.' : 'The supplied template configuration is preserved.',
    included.length ? `Included: ${included.join(', ')}. Plugins and routines need setup before use.` : '',
    'Schedules stay paused until you enable them.'].filter(Boolean).join(' ')
}
