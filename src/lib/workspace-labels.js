export function folderLabel(path) {
  return path?.replace(/[\\/]+$/, '').split(/[\\/]/).at(-1) || path || ''
}

export function browserLabel(address) {
  if (!address) return ''
  try {
    const url = new URL(address)
    if (!['http:', 'https:'].includes(url.protocol)) return address
    return url.host.replace(/^www\./, '') + url.pathname + url.search + url.hash
  } catch { return address }
}
