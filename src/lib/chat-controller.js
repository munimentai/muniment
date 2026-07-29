import { applyBufferedChatEvents, applyChatEvent, historyMessages } from './chat-state.js'

const settledPhases = new Set(['complete', 'cancelled', 'failed', 'interrupted'])
const historyPageCap = 100
const historyPageLimit = 100

export function createChatController({
  invoke,
  listen,
  readMessages,
  readActive,
  readAnnounced,
  readDraft,
  readFiles,
  readThreadId = () => null,
  readThreadSummaries = () => [],
  blocked = () => false,
  onMessages,
  onActive,
  onAnnounce,
  onDraft,
  onFiles,
  onSubmitError,
  onCancelError,
  onQueueError,
  onHistoryError,
  onHistoryStart = () => {},
  onThreadSummaries = () => {},
  onThreadSelected = () => {},
  onThreadSwitch = () => {},
  onFreshThread = () => {},
  onHistoryLoaded = () => {},
  onFollow = () => {},
  onFocus = () => {},
  onSend = () => {},
}) {
  const buffered = new Map()
  let submissionSequence = 0
  let unlisten
  let destroyed = false
  let switchingThread = false
  let switchBlocked = false
  let threadRefreshSequence = 0
  const renameQueues = new Map()

  const messages = () => readMessages()
  const active = () => readActive()
  const publishMessages = (next) => {
    if (!destroyed) onMessages(next)
  }

  async function refreshThreads() {
    const sequence = ++threadRefreshSequence
    try {
      const [{ summaries }, currentThreadId] = await Promise.all([
        invoke('chat_thread_summaries', { limit: 20 }),
        invoke('chat_current_thread'),
      ])
      if (destroyed || sequence !== threadRefreshSequence) return
      onThreadSummaries(summaries)
      onThreadSelected(currentThreadId)
      if (currentThreadId && summaries.some((summary) => summary.threadId === currentThreadId)) {
        onFreshThread(false)
      }
    } catch (_) {
      // A settlement refresh must not disturb the visible conversation state.
    }
  }

  function handleEvent({ payload }) {
    const current = messages().find((message) => message.run?.id === payload.runId)?.run
    if (!current) {
      buffered.set(payload.runId, [...(buffered.get(payload.runId) ?? []), payload])
      return
    }
    const projected = applyChatEvent(current, payload)
    if (projected) publishMessages(messages().map((message) => message.run?.id === projected.id ? { ...message, run: projected } : message))
    if (projected && (!settledPhases.has(current.phase) || readAnnounced()?.id === payload.runId)) onAnnounce(projected)
    if (active()?.id === payload.runId) {
      const settled = projected && settledPhases.has(projected.phase)
      onActive(settled ? null : projected)
      if (settled) void refreshThreads()
    }
  }

  async function start() {
    const stop = await listen('chat-event', handleEvent)
    if (!stop) return
    if (destroyed) stop()
    else unlisten = stop
  }

  async function loadHistory() {
    onHistoryError('')
    onHistoryStart()
    onAnnounce(null)
    try {
      const { summaries } = await invoke('chat_thread_summaries', { limit: 20 })
      if (destroyed) return
      onThreadSummaries(summaries)
      if (!summaries.length) {
        publishMessages([])
        onThreadSelected(null)
        onFreshThread(true)
        onHistoryLoaded()
        onFollow()
        return
      }
      await openThread(summaries[0].threadId, switchBlocked)
    } catch (_) {
      if (!destroyed) onHistoryError('Conversation history could not be restored. Try again.')
    }
  }

  async function openThread(threadId, select = false) {
    if (active() || switchingThread) return
    threadRefreshSequence += 1
    switchingThread = true
    onThreadSwitch(true)
    onHistoryError('')
    const previousThreadId = readThreadId()
    const wasBlocked = switchBlocked
    let selected = false
    try {
      if (select) {
        await invoke('chat_select_thread', { threadId })
        selected = true
      }
      const history = []
      let cursor
      for (let page = 0; page < historyPageCap; page += 1) {
        const payload = { threadId, limit: historyPageLimit }
        if (cursor !== undefined) payload.cursor = cursor
        const result = await invoke('chat_thread_open', payload)
        if (destroyed) return
        history.push(...result.entries)
        if (result.nextCursor == null) break
        cursor = result.nextCursor
      }
      if (destroyed) return
      onHistoryStart()
      onAnnounce(null)
      onThreadSelected(threadId)
      onFreshThread(false)
      publishMessages(historyMessages(history))
      onHistoryLoaded()
      onFollow()
      switchBlocked = false
    } catch (_) {
      if (!destroyed) {
        if (wasBlocked) {
          switchBlocked = true
        } else if (selected) {
          if (previousThreadId == null) {
            switchBlocked = true
          } else {
            try {
              await invoke('chat_select_thread', { threadId: previousThreadId })
              switchBlocked = false
            } catch (_) {
              switchBlocked = true
            }
          }
        }
        onHistoryError('Conversation history could not be restored. Try again.')
      }
    } finally {
      switchingThread = false
      if (!destroyed) onThreadSwitch(switchBlocked)
    }
  }

  async function newThread() {
    if (active() || switchingThread || switchBlocked) return
    threadRefreshSequence += 1
    switchingThread = true
    onThreadSwitch(true)
    onHistoryError('')
    try {
      await invoke('chat_new_thread')
      if (destroyed) return
      onHistoryStart()
      onAnnounce(null)
      onThreadSelected(null)
      onFreshThread(true)
      publishMessages([])
      onHistoryLoaded()
      onFollow()
      onFocus()
    } catch (_) {
      if (!destroyed) onHistoryError('A new thread could not be started. Try again.')
    } finally {
      switchingThread = false
      if (!destroyed) onThreadSwitch(switchBlocked)
    }
  }

  async function renameThread(title, previousTitle) {
    const threadId = readThreadId()
    const trimmed = title.trim()
    if (!threadId || !trimmed || trimmed === previousTitle) return false
    const previousRename = renameQueues.get(threadId) ?? Promise.resolve()
    const rename = previousRename.then(async () => {
      if (destroyed) return false
      onHistoryError('')
      try {
        await invoke('chat_rename_thread', { threadId, title: trimmed })
        if (destroyed) return false
        onThreadSummaries(readThreadSummaries().map((summary) => (
          summary.threadId === threadId ? { ...summary, title: trimmed } : summary
        )))
        return true
      } catch (_) {
        if (!destroyed) {
          onHistoryError('The thread name could not be changed. Try again.')
        }
        return false
      }
    })
    renameQueues.set(threadId, rename)
    try {
      return await rename
    } finally {
      if (renameQueues.get(threadId) === rename) renameQueues.delete(threadId)
    }
  }

  async function deleteThread(threadId) {
    if (!threadId || active() || switchingThread || switchBlocked) return false
    const deletingCurrent = threadId === readThreadId()
    threadRefreshSequence += 1
    switchingThread = true
    onThreadSwitch(true)
    onHistoryError('')
    try {
      await invoke('chat_delete_thread', { threadId })
      if (destroyed) return false
      onThreadSummaries(readThreadSummaries().filter((summary) => summary.threadId !== threadId))
      if (deletingCurrent) {
        onHistoryStart()
        onAnnounce(null)
        onThreadSelected(null)
        onFreshThread(true)
        publishMessages([])
        onHistoryLoaded()
        onFollow()
        onFocus()
      }
      return true
    } catch (_) {
      if (!destroyed) onHistoryError('The thread could not be deleted. Try again.')
      return false
    } finally {
      switchingThread = false
      if (!destroyed) onThreadSwitch(switchBlocked)
    }
  }

  async function send() {
    const prompt = readDraft().trim()
    if (!prompt || active() || switchingThread || switchBlocked || blocked()) return
    onSend()
    onSubmitError('')
    const submissionId = ++submissionSequence
    const userMessage = { role: 'user', text: prompt, attachments: [], submissionId }
    const pending = { id: 'pending', phase: 'thinking', text: '', receipt: null, prompt, submissionId }
    publishMessages([...messages(), userMessage, { role: 'assistant', run: pending }])
    onActive(pending)
    onAnnounce(pending)
    onFollow()
    try {
      const run = await invoke('chat_submit', {
        prompt,
        files: readFiles().map(({ path }) => ({ path })),
      })
      if (destroyed || submissionId !== submissionSequence) return
      onDraft('')
      onFiles([])
      publishMessages(messages().map((message) => message.submissionId === submissionId ? { ...message, attachments: run.attachments ?? [] } : message))
      const identified = { ...pending, id: run.runId }
      const projected = applyBufferedChatEvents(identified, buffered.get(run.runId) ?? [])
      buffered.delete(run.runId)
      publishMessages(messages().map((message) => message.run?.submissionId === submissionId ? { ...message, run: projected } : message))
      onAnnounce(projected)
      const settled = settledPhases.has(projected.phase)
      onActive(settled ? null : projected)
      if (settled) await refreshThreads()
    } catch (error) {
      if (destroyed) return
      const failed = { ...pending, id: `rejected-${messages().length}`, phase: 'failed' }
      publishMessages(messages().map((message) => message.run?.submissionId === submissionId ? { ...message, run: failed } : message))
      if (submissionId !== submissionSequence) return
      onAnnounce(failed)
      onSubmitError(typeof error === 'string' ? error : 'The message could not be sent. Try again.')
      onActive(null)
    }
  }

  async function cancel() {
    const run = active()
    if (!run) return
    onCancelError('')
    try {
      await invoke('chat_cancel', { runId: run.id })
    } catch (_) {
      if (!destroyed) onCancelError('Could not stop this reply. Try again.')
    }
  }

  async function resume(run) {
    if (active() || blocked() || !run.resumable || run.phase !== 'interrupted') return
    const resuming = { ...run, phase: 'resuming', resumeError: '' }
    onActive(resuming)
    onAnnounce(resuming)
    publishMessages(messages().map((message) => message.run?.id === run.id ? { ...message, run: resuming } : message))
    try {
      await invoke('chat_resume', { runId: run.id })
      if (destroyed) return
      const current = messages().find((message) => message.run?.id === run.id)?.run ?? resuming
      const projected = applyBufferedChatEvents(current, buffered.get(run.id) ?? [])
      buffered.delete(run.id)
      publishMessages(messages().map((message) => message.run?.id === run.id ? { ...message, run: projected } : message))
      onAnnounce(projected)
      const settled = settledPhases.has(projected.phase)
      onActive(settled ? null : projected)
      if (settled) await refreshThreads()
    } catch (error) {
      if (destroyed) return
      const interrupted = { ...run, phase: 'interrupted', resumeError: typeof error === 'string' ? error : 'This reply could not be resumed. Try again.' }
      publishMessages(messages().map((message) => message.run?.id === run.id ? { ...message, run: interrupted } : message))
      onAnnounce(interrupted)
      onActive(null)
    }
  }

  async function queue(delivery) {
    const message = readDraft().trim()
    const run = active()
    if (!message || !run || run.id === 'pending' || blocked()) return
    onQueueError('')
    try {
      await invoke('chat_queue', { runId: run.id, delivery, message })
      if (destroyed) return
      publishMessages([...messages(), { role: 'user', text: message }])
      onFollow()
      if (readDraft().trim() === message) onDraft('')
    } catch (error) {
      if (!destroyed) onQueueError(typeof error === 'string' ? error : String(error))
    }
  }

  function cleanup() {
    destroyed = true
    threadRefreshSequence += 1
    unlisten?.()
    unlisten = undefined
    buffered.clear()
  }

  return { start, loadHistory, openThread: (threadId) => openThread(threadId, true), newThread, renameThread, deleteThread, send, cancel, resume, queue, cleanup }
}
