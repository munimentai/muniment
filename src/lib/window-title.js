const APP_TITLE = 'muniment'

export function formatWindowTitle(threadTitle) {
  return threadTitle ? `${threadTitle} | ${APP_TITLE}` : APP_TITLE
}

export function createWindowTitle(windowApi) {
  async function set(threadTitle) {
    try {
      await windowApi?.setTitle(formatWindowTitle(threadTitle))
    } catch (_) {}
  }

  return { set }
}
