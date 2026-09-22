export function folderLabel(path) {
  return path?.replace(/[\\/]+$/, '').split(/[\\/]/).at(-1) || path || ''
}

export function workspaceLabel(path, context) {
  const name = folderLabel(path)
  return context?.threadId && /-[a-f0-9]{8}$/i.test(name) ? name.replace(/-[a-f0-9]{8}$/i, '').replaceAll('_', ' ') : name
}

export function browserLabel(address) {
  if (!address) return ''
  try {
    const url = new URL(address)
    if (url.hostname === '127.0.0.1' && url.pathname === '/docs/') return 'Muniment docs'
    if (!['http:', 'https:'].includes(url.protocol)) return address
    return url.host.replace(/^www\./, '') + url.pathname + url.search + url.hash
  } catch { return address }
}
