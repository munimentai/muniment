const approvedProtocols = new Set(['https:', 'http:', 'mailto:'])

export function createExternalLinkHandler(open) {
  return function handleExternalLink(event) {
    const link = event.target?.closest?.('a')
    if (!link || (event.currentTarget && !event.currentTarget.contains(link))) return

    event.preventDefault()

    const address = link.getAttribute('href')
    let url
    try {
      url = new URL(address)
    } catch (_) {
      return
    }

    if (!approvedProtocols.has(url.protocol)) return
    return Promise.resolve().then(() => open(address)).catch(() => undefined)
  }
}
