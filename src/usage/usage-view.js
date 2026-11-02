// The Usage page's view for the app session: its tab, range, filters and
// breakdown. The workspace redraws the page when the runtime switches, such
// as to the child runtime, and the page reads its view back from here, so a
// reconnect never returns it to Limits.
const DEFAULT_VIEW = { view: 'limits', range: '7d', machine: '', provider: '', breakdown: 'model' }
let saved = { ...DEFAULT_VIEW }

export const usageView = () => ({ ...saved })
export function keepUsageView(next) { saved = { ...saved, ...next } }
export function resetUsageView() { saved = { ...DEFAULT_VIEW } }
