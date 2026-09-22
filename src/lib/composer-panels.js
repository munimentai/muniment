export const COMPOSER_PANEL_EVENT = 'muniment:composer-panel'
export function openComposerPanel(panel) {
  window.dispatchEvent(new CustomEvent(COMPOSER_PANEL_EVENT, { detail: panel }))
}
