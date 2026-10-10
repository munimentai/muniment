import { featureFlags } from '../feature-flags.js'

// The Settings pages in sidebar order, under their group headings. Each page is
// [id, label, icon]: a Lucide name, or `mcp` for the MCP mark. The cloud flag adds Account and the company-record
// flag adds Companies.
export function settingsGroups() {
  return [
    ['Models', [['providers', 'Model providers', 'key-round'], ['model-list', 'Model list', 'cpu'], ['decisions', 'Decision models', 'route']]],
    ['Extend', [['mcp', 'MCP servers', 'mcp'], ['skills', 'Skills', 'pencil-sparkles'], ['plugins', 'Plugins', 'unplug']]],
    ['Preferences', [['appearance', 'Appearance', 'palette'], ['context', 'Context', 'messages-square'], ['language', 'Language', 'languages']]],
    ['Personal', [['profile', 'Profile', 'user'], ['memory', 'Memory', 'brain'], ...(featureFlags.cloud ? [['account', 'Account', 'circle-user']] : [])]],
    ['Workspace', [['home', 'Storage', 'folders'], ...(featureFlags.companyRecord ? [['companies', 'Companies', 'building-2']] : [])]],
  ]
}

export function settingsSections() {
  return settingsGroups().flatMap(([, pages]) => pages)
}

// Callers name a page by its id, or by the area that holds it.
const ALIASES = { models: 'providers', routing: 'decisions', extend: 'mcp' }
export function settingsPage(id) {
  return ALIASES[id] ?? id
}

// Models and Extend keep one component each, and the sidebar picks its tab.
export const MODEL_TABS = { providers: 'accounts', 'model-list': 'models', decisions: 'routing' }
export const EXTEND_TABS = { mcp: 'mcp', skills: 'skill', plugins: 'plugin' }
export function pageForTab(tabs, tab) {
  return Object.keys(tabs).find((page) => tabs[page] === tab)
}
