// An allowance window's short name: the period alone, then the model it covers
// when the upstream scopes it, as in "Weekly" and "Weekly Fable".
export function allowanceLabel(window) {
  const period = String(window?.label ?? '').replace(/\s*window$/i, '') || 'Allowance'
  return window?.scope ? `${period} ${window.scope}` : period
}
