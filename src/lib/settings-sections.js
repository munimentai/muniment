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
    ['voice', 'Voice', 'mic'],
    ['general', 'General', 'sliders-horizontal'],
  ]
}

// The parts of a page that search finds and a link can open: [page, part,
// label, keywords]. A part names a block of its page, such as the Skills
// filter of Tools or the Context block of Data. The keywords hold every
// heading and row title the part shows and the words people type for them,
// and a test holds each page to that. The cloud flag adds the MuniCloud
// account and the company-record flag adds Companies.
export function settingsIndex({ remote = false } = {}) {
  return [
    ['account', 'profile', 'Profile', 'about you name preferred name work instructions load markdown save profile me personal bio'],
    ...(featureFlags.cloud && !remote ? [
      ['account', 'cloud', 'MuniCloud account', 'municloud signed in sign in sign out log in log out login logout plan plans billing subscription cloud access connected programs free'],
      ['account', 'vault', 'Keep model connections in MuniCloud', 'shared vault sharing cloud'],
    ] : []),
    ['models', 'roles', 'Roles', 'routing router route each message decision model assistance assist default statements'],
    ['models', 'connections', 'Connections', 'providers provider accounts account subscriptions subscription sign in log in login api keys api key local server local models local model ollama omlx lm studio endpoint connect refresh claude anthropic openai chatgpt codex gemini google grok xai openrouter'],
    ['tools', 'mcp', 'MCP servers', 'mcps connectors connector servers server catalog custom popular mcps more mcps integrations integration extensions extension'],
    ['tools', 'skill', 'Skills', 'skills skill install installed create from chat folder extensions extension'],
    ['tools', 'plugin', 'Plugins', 'plugins plugin install installed package packages extensions extension'],
    ['tools', 'search', 'Search', 'web search providers provider engines engine search engines search order muniment search engines add a search provider browser search address bar engine keys key brave duckduckgo wikipedia yahoo bing google searxng'],
    ['machines', 'list', 'Machines', 'devices device computers computer this computer containers container docker servers server phones phone remote fleet sync syncing update all version account'],
    ['machines', 'add', 'Add machine', 'code qr link ssh connect another computer pair pairing a phone add device'],
    ['machines', 'share', 'Share model connections', 'sharing subscriptions keys'],
    ['machines', 'this-computer', 'This computer', 'available offline network port addresses accept connections log'],
    ['permissions', 'modes', 'Default modes', 'permissions permission new threads agents modes mode bypass permissions allow bypass ask ask before changes plan plan first auto review don\'t ask dont ask approvals approval approve safety protected paths unattended runs wait for an answer'],
    ['data', 'memory', 'Memory', 'saved memories memories memory facts fact recall export import imports deleted save facts from chats ask before saving use memories in replies backup zip'],
    ['data', 'context', 'Context', 'compaction compact tokens recent messages room'],
    ['data', 'retention', 'Thread retention', 'keep delete data threads days year history'],
    ['data', 'storage', 'Storage', 'workspace folder home files folders disk location'],
    ...(featureFlags.companyRecord ? [['data', 'companies', 'Companies', 'company record records sources sample backups backup back up company records daily keep backups for your companies zip export']] : []),
    ['voice', 'shortcut', 'Shortcut', 'dictation shortcut shortcut behavior dictate hold to talk push to talk press twice hands-free hands free hotkey hot key keyboard cancel dictation escape speech to text'],
    ['voice', 'microphone', 'Microphone', 'mic mics microphones input device mute other audio while dictating start and stop sounds sound'],
    ['voice', 'speech-model', 'Speech model', 'speech to text transcribe dictation model models parakeet on-device on device download install voice activity english'],
    ['voice', 'transcription', 'Transcription', 'transcribe transcript remove filler words um uh send when dictation stops custom words names terms vocabulary dictionary spelling'],
    ['general', 'appearance', 'Appearance', 'theme themes mode dark mode light mode dark light night system frame framed frameless colors color colour scheme color scheme style normal vibrant contrast high contrast accent look agent avatars avatar'],
    ['general', 'type', 'Type', 'fonts font text size bigger text smaller text larger text font size typeface conversation headers records'],
    ['general', 'display', 'Display', 'zoom scale density spacing compact comfortable padding'],
    ['general', 'motion', 'Motion', 'animation animations reduce motion reduced motion movement transitions transition accessibility'],
    ['general', 'thinking', 'Thinking indicator', 'thinking indicator spinner mark activity working'],
    ['general', 'language', 'Language', 'browser language languages translation translate locale'],
    ['general', 'time', 'Time', 'time zone and clock clocks time zone time zones timezone timezones zone 12-hour 24-hour 12 hour 24 hour am pm military time date format hour cycle'],
    ['general', 'notifications', 'Notifications', 'system notifications notification alerts alert sounds sound banners banner badges badge needs you finished approvals questions failures usage limit bell'],
    ['general', 'about', 'About', 'version muniment update updates'],
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
  retention: ['data', 'retention'],
  companies: ['data', 'companies'],
  appearance: ['general', 'appearance'],
  type: ['general', 'appearance'],
  display: ['general', 'appearance'],
  motion: ['general', 'appearance'],
  thinking: ['general', 'appearance'],
  voice: ['voice', null],
  time: ['general', 'time'],
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
