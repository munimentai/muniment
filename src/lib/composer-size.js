// Design-spec §4: the composer input rests at two rows, grows with the draft to
// a ten-line cap, and only then scrolls internally. The arithmetic lives here;
// App.svelte only measures the textarea and applies the result.
export const COMPOSER_RESTING_ROWS = 2
export const COMPOSER_MAX_ROWS = 10

export function composerHeight({
  contentHeight,
  lineHeight,
  padding = 0,
  restingRows = COMPOSER_RESTING_ROWS,
  maxRows = COMPOSER_MAX_ROWS,
}) {
  // `line-height: normal` and unstyled test environments both measure as NaN.
  // Without a usable row height there is no honest clamp, so report no height
  // and let the caller fall back to the textarea's own `rows` sizing rather
  // than collapsing the box to the padding.
  if (!Number.isFinite(lineHeight) || lineHeight <= 0) return { height: null, capped: false }

  // The textarea carries no border (that lives on .composer), so a border-box
  // scrollHeight is exactly the text plus its vertical padding.
  const frame = Number.isFinite(padding) && padding > 0 ? padding : 0
  const resting = Math.ceil(restingRows * lineHeight + frame)
  const cap = Math.ceil(Math.max(restingRows, maxRows) * lineHeight + frame)
  const measured = Number.isFinite(contentHeight) ? Math.ceil(contentHeight) : resting

  return {
    height: Math.min(cap, Math.max(resting, measured)),
    capped: measured >= cap,
  }
}
