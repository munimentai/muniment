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

export function createStreamingUnderlineAction(tick) {
  return function streamingUnderline(node) {
    let mounted = true
    function measure() {
      const caret = node.querySelector('.caret')
      const rule = node.querySelector('.streaming-rule')
      if (!caret || !rule) return
      const geometry = streamingUnderlineGeometry({
        caretLeft: caret.offsetLeft,
        caretTop: caret.offsetTop,
        caretHeight: caret.offsetHeight,
      })
      if (!geometry) return
      rule.style.left = `${geometry.left}px`
      rule.style.top = `${geometry.top}px`
      rule.style.width = `${geometry.width}px`
    }

    function measureAfterRender() {
      tick().then(() => {
        if (mounted) measure()
      })
    }

    measureAfterRender()
    document.fonts?.ready?.then(() => {
      if (mounted) measure()
    })
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(measure)
    observer?.observe(node)
    return {
      update: measureAfterRender,
      destroy: () => {
        mounted = false
        observer?.disconnect()
      },
    }
  }
}
