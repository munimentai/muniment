export function formatBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes < 0) return 'Unknown size'
  if (bytes < 1024) return `${bytes} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let value = bytes / 1024
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`
}

const stateWords = {
  notInstalled: 'Not installed.',
  installing: 'Installing.',
  installed: 'Installed. Press Voice again to dictate.',
  cancelled: 'Cancelled.',
  failed: 'Failed.',
}

export function installStateWords(state) {
  return stateWords[state] ?? 'Install state unavailable.'
}
