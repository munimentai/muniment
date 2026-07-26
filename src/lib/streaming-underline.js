// The browser supplies the caret's inline-fragment position; this helper keeps
// the decision about the active-line rule independent of DOM layout.
export function streamingUnderlineGeometry({ caretLeft, caretTop, caretHeight }) {
  if (![caretLeft, caretTop, caretHeight].every(Number.isFinite)) return null
  if (caretLeft < 0 || caretTop < 0 || caretHeight <= 0) return null

  return {
    left: 0,
    top: caretTop + caretHeight,
    width: caretLeft,
  }
}
