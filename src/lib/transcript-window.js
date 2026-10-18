// The transcript renders a window of its items, so a thread of fifty thousand
// turns opens with the work of a short one. The window holds the newest items
// and follows new ones while the reader sits at the end. Scrolling near the top
// adds older items above, and near the bottom adds newer items below. The
// window never holds more than WINDOW_MAX items, so the far side drops items as
// the near side gains them, and the DOM stays bounded however far the reader goes.
export const WINDOW_SIZE = 40
export const WINDOW_STEP = 40
export const WINDOW_MAX = 120
// The distance from an edge, in CSS pixels, at which the window grows.
export const WINDOW_EDGE = 800

// `end` is null while the window follows the newest item, else the index one
// past its last item. `size` is the number of items it holds.
export const tailWindow = () => ({ end: null, size: WINDOW_SIZE })

export function windowBounds(length, view = tailWindow()) {
  const end = view.end == null ? length : Math.min(view.end, length)
  return { start: Math.max(0, end - view.size), end }
}

export function windowItems(items, view) {
  const { start, end } = windowBounds(items.length, view)
  return start === 0 && end === items.length ? items : items.slice(start, end)
}

const settle = (length, start, end) => ({ end: end >= length ? null : end, size: end - start })

// Older items above: the start moves up a step, and past the cap the end follows it.
export function earlier(length, view) {
  const { start, end } = windowBounds(length, view)
  if (start === 0) return view
  const nextStart = Math.max(0, start - WINDOW_STEP)
  return settle(length, nextStart, Math.min(end, nextStart + WINDOW_MAX))
}

// Newer items below: the end moves down a step, and past the cap the start follows it.
export function later(length, view) {
  if (view.end == null) return view
  const { start, end } = windowBounds(length, view)
  const nextEnd = Math.min(length, end + WINDOW_STEP)
  return settle(length, Math.max(start, nextEnd - WINDOW_MAX), nextEnd)
}

// A reader who leaves the end keeps the items in view: the window stops
// following, so an arriving turn never pushes a rendered one out above them.
export function freeze(length, view) {
  if (view.end != null) return view
  const { start, end } = windowBounds(length, view)
  return { end, size: end - start }
}

export const sameWindow = (first, second) => first.end === second.end && first.size === second.size

// Grows the window from the thread's scroll position. The transcript marks its
// item list with two `.transcript-edge` elements, and the item next to the
// edge that keeps its place anchors the scroll, so content added above or
// dropped below never moves what the reader sees.
export function createTranscriptWindow({ readThread, readLength, readPinned, readWindow, onWindow, onScrollTop = () => {}, tick }) {
  let moving = false

  async function move(step) {
    const thread = readThread()
    const current = readWindow()
    const next = step(readLength(), current)
    if (moving || !thread || sameWindow(current, next)) return false
    const edges = thread.querySelectorAll(':scope > .transcript-edge')
    const anchor = step === earlier ? edges[0]?.nextElementSibling : edges[1]?.previousElementSibling
    const usable = anchor && !anchor.classList.contains('transcript-edge')
    const before = usable ? anchor.getBoundingClientRect().top : 0
    moving = true
    try {
      onWindow(next)
      await tick()
      if (usable && anchor.isConnected) {
        const shift = anchor.getBoundingClientRect().top - before
        if (shift) thread.scrollTop += shift
      }
      onScrollTop(thread.scrollTop)
    } finally {
      moving = false
    }
    return true
  }

  // Called after the transcript's own scroll handling.
  function scrolled() {
    const thread = readThread()
    if (!thread) return
    const below = thread.scrollHeight - thread.scrollTop - thread.clientHeight
    if (!readPinned() && below >= WINDOW_EDGE && readWindow().end == null) onWindow(freeze(readLength(), readWindow()))
    if (thread.scrollTop < WINDOW_EDGE) return move(earlier)
    if (below < WINDOW_EDGE) return move(later)
  }

  // New items below a window that stopped following show once the reader is near them.
  function grew() {
    const thread = readThread()
    if (!thread || readWindow().end == null) return
    if (thread.scrollHeight - thread.scrollTop - thread.clientHeight < WINDOW_EDGE) return move(later)
  }

  return { scrolled, grew, move }
}
