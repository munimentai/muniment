import { COPY_CONFIRMATION_MS, copyResult } from './message-actions.js'
import { scrollFollowState } from './scroll-follow.js'

// Each transcript entry's key. A user message keys on its run id or its
// submission, and a reply on its submission or its run id, so a pending reply
// keeps its key when the runtime names the run. A repeated key gains its position.
export function transcriptKeys(messages = []) {
  const keys = new Map()
  const seen = new Set()
  messages.forEach((message, index) => {
    const run = message.run
    let key = message.role === 'user'
      ? `user:${message.id ?? (message.submissionId != null ? `s${message.submissionId}` : `i${index}`)}`
      : `run:${run?.submissionId != null ? `s${run.submissionId}` : run?.id ?? `i${index}`}`
    if (seen.has(key)) key = `${key}:${index}`
    seen.add(key)
    keys.set(message, key)
  })
  return keys
}

export function createChatTranscriptController({
  tick,
  clipboard,
  readThread,
  readPinned,
  readDestroyed,
  onPinned,
  onContentBelow,
  onCopy,
  readExpandedReceipts,
  onExpandedReceipts,
  readParallelTools,
  onParallelTools,
  frame = (callback) => typeof requestAnimationFrame === 'function' ? requestAnimationFrame(callback) : setTimeout(callback, 16),
}) {
  let lastScrollTop = 0
  let following = null
  let copyEpoch = 0
  let copyTimer

  function scrollToLatest() {
    const thread = readThread()
    if (!thread) return
    onPinned(true)
    const behavior = window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth'
    thread.scrollTo({ top: thread.scrollHeight, behavior })
    lastScrollTop = thread.scrollHeight - thread.clientHeight
    onContentBelow(false)
  }

  // Streamed text calls this for every token. The calls in one frame share one
  // scroll, so the thread lays out once per frame and not once per token.
  function followNewContent() {
    if (!readPinned()) return
    following ??= tick().then(() => new Promise((resolve) => frame(() => {
      following = null
      const thread = readThread()
      if (readPinned() && thread && !readDestroyed()) {
        thread.scrollTo({ top: thread.scrollHeight, behavior: 'auto' })
        lastScrollTop = thread.scrollHeight - thread.clientHeight
        onContentBelow(false)
      }
      resolve()
    })))
    return following
  }

  function handleScroll() {
    const thread = readThread()
    const next = scrollFollowState({
      pinned: readPinned(),
      scrollTop: thread.scrollTop,
      scrollHeight: thread.scrollHeight,
      clientHeight: thread.clientHeight,
      lastScrollTop,
    })
    onPinned(next.pinned)
    lastScrollTop = next.lastScrollTop
    onContentBelow(!next.pinned && thread.scrollHeight - thread.clientHeight - thread.scrollTop > 0)
  }

  function syncScrollTop(scrollTop) {
    lastScrollTop = scrollTop
  }

  async function copyResponse(run) {
    clearTimeout(copyTimer)
    const epoch = ++copyEpoch
    let result
    try {
      await clipboard.writeText(run.text ?? '')
      result = copyResult(run.id, true)
    } catch (_) {
      result = copyResult(run.id, false)
    }
    if (readDestroyed() || epoch !== copyEpoch) return
    onCopy(null)
    await tick()
    if (readDestroyed() || epoch !== copyEpoch) return
    onCopy(result)
    if (result.status !== 'copied') return
    copyTimer = setTimeout(() => {
      if (!readDestroyed() && epoch === copyEpoch) onCopy(null)
    }, COPY_CONFIRMATION_MS)
  }

  function toggleReceipt(runId) {
    const next = new Set(readExpandedReceipts())
    next.has(runId) ? next.delete(runId) : next.add(runId)
    onExpandedReceipts(next)
  }

  function trackParallelTools(messages) {
    const current = readParallelTools()
    const next = new Map(current)
    for (const message of messages) {
      if (message.role !== 'assistant') continue
      const running = (message.run.toolActivity ?? []).filter((tool) => tool.status === 'running')
      if (running.length > 1) {
        const grouped = new Set(next.get(message.run.id) ?? [])
        running.forEach((tool) => grouped.add(tool.effectId))
        next.set(message.run.id, [...grouped])
      }
    }
    if ([...next].some(([id, tools]) => tools.length !== (current.get(id)?.length ?? 0))) onParallelTools(next)
  }

  function cleanup() {
    clearTimeout(copyTimer)
  }

  return { scrollToLatest, followNewContent, handleScroll, syncScrollTop, copyResponse, toggleReceipt, trackParallelTools, cleanup }
}
