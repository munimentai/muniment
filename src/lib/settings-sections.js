import { featureFlags } from '../feature-flags.js'

// The Settings pages in sidebar order, under their group headings. Each page is
// [id, label, icon]: a Lucide name, or `mcp` for the MCP mark. The cloud flag adds Account and the company-record
// flag adds Companies, the one Workspace page. A paired runtime refuses the cloud account, so `remote` hides Account.
export function settingsGroups({ remote = false } = {}) {
  return [
    ['Models', [['providers', 'Model providers', 'key-round'], ['decisions', 'Decision models', 'route']]],
    ['Extend', [['mcp', 'MCP servers', 'mcp'], ['skills', 'Skills', 'pencil-sparkles'], ['plugins', 'Plugins', 'plug-zap'], ['runtimes', 'Machines', 'network']]],
    ['Preferences', [['appearance', 'Appearance', 'palette'], ['context', 'Context', 'messages-square'], ['permissions', 'Permissions', 'shield-check'], ['voice', 'Voice', 'mic'], ['search', 'Search', 'search'], ['language', 'Language', 'languages'], ['notifications', 'Notifications', 'bell']]],
    ['Personal', [['profile', 'Profile', 'user'], ['memory', 'Memory', 'brain'], ...(featureFlags.cloud && !remote ? [['account', 'Account', 'circle-user']] : []), ['home', 'Storage', 'folders']]],
    ['Workspace', featureFlags.companyRecord ? [['companies', 'Companies', 'building-2']] : []],
  ].filter(([, pages]) => pages.length)
}

export function settingsSections(options) {
  return settingsGroups(options).flatMap(([, pages]) => pages)
}

// Callers name a page by its id, or by the area that holds it.
const ALIASES = { models: 'providers', routing: 'decisions', extend: 'mcp' }
export function settingsPage(id) {
  return ALIASES[id] ?? id
}

// Models and Extend keep one component each, and the sidebar picks its tab.
export const MODEL_TABS = { providers: 'accounts', decisions: 'routing' }
export const EXTEND_TABS = { mcp: 'mcp', skills: 'skill', plugins: 'plugin' }
export function pageForTab(tabs, tab) {
  return Object.keys(tabs).find((page) => tabs[page] === tab)
}
