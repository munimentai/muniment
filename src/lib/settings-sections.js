import { featureFlags } from '../feature-flags.js'

// The Settings pages in sidebar order. Each page is [id, label, icon], with a
// Lucide icon name. Every setting lives on one page.
export function settingsSections() {
  return [
    ['account', 'Account', 'circle-user'],
    ['models', 'Models', 'key-round'],
    ['tools', 'Tools', 'wrench'],
    ['machines', 'Machines', 'network'],
    ['permissions', 'Permissions', 'shield-check'],
    ['data', 'Data', 'database'],
    ['general', 'General', 'sliders-horizontal'],
  ]
}

// The parts of a page that search finds and a link can open: [page, part,
// label, keywords]. A part names a block of its page, such as the Skills
// filter of Tools or the Context block of Data. The cloud flag adds the
// MuniCloud account and the company-record flag adds Companies.
export function settingsIndex({ remote = false } = {}) {
  return [
    ['account', 'profile', 'Profile', 'name preferred work instructions about you load markdown'],
    ...(featureFlags.cloud && !remote ? [
      ['account', 'cloud', 'MuniCloud account', 'sign in sign out plan devices access phone connected programs retention'],
      ['account', 'vault', 'Keep model connections in MuniCloud', 'shared vault sharing cloud'],
    ] : []),
    ['models', 'roles', 'Roles', 'routing route each message decision model assistance assist default statements'],
    ['models', 'connections', 'Connections', 'providers accounts subscription sign in api key local server ollama endpoint connect refresh'],
    ['tools', 'mcp', 'MCP servers', 'mcp connectors servers catalog custom'],
    ['tools', 'skill', 'Skills', 'skills install create from chat folder'],
    ['tools', 'plugin', 'Plugins', 'plugins install package'],
    ['tools', 'search', 'Search', 'web search providers engines order key address bar browser'],
    ['machines', 'list', 'Machines', 'devices this computer container server phone update all version'],
    ['machines', 'add', 'Add machine', 'code qr link ssh connect another computer'],
    ['machines', 'share', 'Share model connections', 'sharing subscriptions keys'],
    ['machines', 'this-computer', 'This computer', 'available offline network port addresses accept connections log'],
    ['permissions', 'modes', 'Default modes', 'new threads agents mode bypass ask'],
    ['permissions', 'rules', 'Rules for every thread', 'allow deny ask first rules unattended wait'],
    ['data', 'memory', 'Memory', 'saved memories facts recall export import deleted'],
    ['data', 'context', 'Context', 'compaction compact tokens recent messages room'],
    ['data', 'storage', 'Storage', 'workspace folder home files'],
    ...(featureFlags.companyRecord ? [['data', 'companies', 'Companies', 'company record sources sample backups']] : []),
    ['general', 'appearance', 'Appearance', 'theme mode dark light frame color scheme type size font avatars thinking indicator'],
    ['general', 'language', 'Language', 'browser language translation'],
    ['general', 'voice', 'Voice', 'dictation shortcut microphone speech model filler words custom words'],
    ['general', 'notifications', 'Notifications', 'system notifications alerts needs you finished approvals questions failures usage limit bell'],
    ['general', 'about', 'About', 'version'],
  ]
}

// The sidebar rows for a search: every page while the search is empty, else
// the pages that match, each with the parts that match under it. A page
// matches by its label, and a part by its label or keywords. Each row is
// [id, label, icon, [[part, label]]].
export function settingsNav(query, options) {
  const words = String(query ?? '').toLowerCase().split(/\s+/).filter(Boolean)
  const pages = settingsSections()
  if (!words.length) return pages.map((page) => [...page, []])
  const hit = (text) => words.every((word) => text.toLowerCase().includes(word))
  const index = settingsIndex(options)
  return pages
    .map(([id, label, icon]) => [id, label, icon, index.filter(([page, , name, keywords]) => page === id && hit(`${name} ${keywords}`)).map(([, part, name]) => [part, name]), hit(label)])
    .filter(([, , , parts, named]) => named || parts.length)
    .map(([id, label, icon, parts]) => [id, label, icon, parts])
}

// Callers name a page by its id, or by an older page or part id that now
// sits inside a page. Each maps to [page, part].
const TARGETS = {
  providers: ['models', 'connections'],
  decisions: ['models', 'roles'],
  routing: ['models', 'roles'],
  extend: ['tools', 'mcp'],
  mcp: ['tools', 'mcp'],
  skills: ['tools', 'skill'],
  skill: ['tools', 'skill'],
  plugins: ['tools', 'plugin'],
  plugin: ['tools', 'plugin'],
  search: ['tools', 'search'],
  runtimes: ['machines', null],
  profile: ['account', 'profile'],
  memory: ['data', 'memory'],
  context: ['data', 'context'],
  home: ['data', 'storage'],
  storage: ['data', 'storage'],
  companies: ['data', 'companies'],
  appearance: ['general', 'appearance'],
  voice: ['general', 'voice'],
  language: ['general', 'language'],
  notifications: ['general', 'notifications'],
}

export function settingsTarget(id) {
  if (TARGETS[id]) return TARGETS[id]
  const index = settingsIndex().find(([, part]) => part === id)
  if (index) return [index[0], id]
  return [id, null]
}

export function settingsPage(id) {
  return settingsTarget(id)[0]
}
