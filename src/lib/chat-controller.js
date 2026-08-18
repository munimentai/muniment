import { applyBufferedChatEvents, applyChatEvent, historyMessages, settledPhases, unsettledRun } from './chat-state.js'

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
  onMoreThreads = () => {},
  onThreadSelected = () => {},
  onThreadSwitch = () => {},
  onFreshThread = () => {},
  onHistoryLoaded = () => {},
  onFollow = () => {},
  onFocus = () => {},
  onSend = () => {},
}) {
  const buffered = new Map()
  // How many calls wait for a run id right now: a submission, a resume, or a
  // thread load. The buffer only spans those waits, and the last one to finish
  // empties it.
  let runIdWaits = 0
  let submissionSequence = 0
  let unlisten
  let registration
  let registrationFailed = false
  let destroyed = false
  let switchingThread = false
  let switchBlocked = false
  let loadingHistory = false
  let threadRefreshSequence = 0
  let threadPageCount = 1
  let nextThreadCursor = null
  let loadingOlderThreads = false
  const renameQueues = new Map()

  function holdBuffer() {
    runIdWaits += 1
  }

  function releaseBuffer() {
    runIdWaits -= 1
    if (!runIdWaits) buffered.clear()
  }

  const messages = () => readMessages()
  const active = () => readActive()
  const publishMessages = (next) => {
    if (!destroyed) onMessages(next)
  }

  async function refreshThreads() {
    const sequence = ++threadRefreshSequence
    try {
      const [firstPage, currentThreadId] = await Promise.all([
        invoke('chat_thread_summaries', { limit: 20 }),
        invoke('chat_current_thread'),
      ])
      if (destroyed || sequence !== threadRefreshSequence) return
      const refreshedThreadIds = new Set(firstPage.summaries.map((summary) => summary.threadId))
      const retainedSummaries = readThreadSummaries().filter((summary) => !refreshedThreadIds.has(summary.threadId))
      const summaries = [...firstPage.summaries, ...retainedSummaries]
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
    // A thread load replaces the whole transcript, so no published run describes
    // this event yet. openThread drains the buffer onto the loaded pages.
    const current = loadingHistory ? null : messages().find((message) => message.run?.id === payload.runId)?.run
    if (!current) {
      // The runtime broadcasts every attach run, so most unknown run ids belong
      // to another surface. Hold the event only while a call waits for its run
      // id. Drop it otherwise, or the map grows for the life of the window.
      if (runIdWaits) buffered.set(payload.runId, [...(buffered.get(payload.runId) ?? []), payload])
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
    if (destroyed || unlisten) return !!unlisten
    if (registration) return registration
    registration = (async () => {
      try {
        const stop = await listen('chat-event', handleEvent)
        if (!stop) return false
        if (destroyed) stop()
        else unlisten = stop
        registrationFailed = false
        return !destroyed
      } catch (_) {
        registrationFailed = true
        if (!destroyed) onHistoryError('Live replies cannot arrive.', { label: 'Reconnect', run: loadHistory })
        return false
      } finally {
        registration = undefined
      }
    })()
    return registration
  }

  async function loadHistory() {
    if (registrationFailed && !await start()) return
    onHistoryStart()
    onAnnounce(null)
    try {
      const { summaries, nextCursor } = await invoke('chat_thread_summaries', { limit: 20 })
      if (destroyed) return
      threadPageCount = 1
      nextThreadCursor = nextCursor
      onThreadSummaries(summaries)
      onMoreThreads(nextThreadCursor != null)
      if (!summaries.length) {
        onHistoryError('')
        publishMessages([])
        onThreadSelected(null)
        onFreshThread(true)
        onHistoryLoaded()
        onFollow()
        return
      }
      await openThread(summaries[0].threadId, switchBlocked)
    } catch (_) {
      if (!destroyed) onHistoryError('Conversation history could not be restored.', { label: 'Restore history', run: loadHistory })
    }
  }

  async function loadOlderThreads() {
    if (destroyed || loadingOlderThreads || nextThreadCursor == null) return null
    loadingOlderThreads = true
    const cursor = nextThreadCursor
    const sequence = threadRefreshSequence
    try {
      const result = await invoke('chat_thread_summaries', { limit: 20, cursor })
      if (destroyed || sequence !== threadRefreshSequence) return null
      const firstThreadId = result.summaries[0]?.threadId ?? null
      onThreadSummaries([...readThreadSummaries(), ...result.summaries])
      threadPageCount += 1
      nextThreadCursor = result.nextCursor
      onMoreThreads(nextThreadCursor != null)
      onHistoryError('')
      return firstThreadId
    } catch (_) {
      if (!destroyed && sequence === threadRefreshSequence) onHistoryError('Older threads could not be loaded.', { label: 'Load older threads', run: loadOlderThreads })
      return null
    } finally {
      loadingOlderThreads = false
    }
  }

  async function openThread(threadId, select = false) {
    if (active() || switchingThread) return
    threadRefreshSequence += 1
    switchingThread = true
    onThreadSwitch(true)
    const previousThreadId = readThreadId()
    const wasBlocked = switchBlocked
    let selected = false
    loadingHistory = true
    holdBuffer()
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
      const published = historyMessages(history)
      // The runtime keeps driving an unsettled run while the window is away. The
      // desktop rejoins that run instead of starting one (ADR 0012, desktop run
      // rejoin). handleEvent carries it forward by run id from here.
      const recorded = unsettledRun(published)
      // handleEvent buffered every event that landed during the load, so the
      // pages loaded over a growing buffer. The recorded state is stale by
      // exactly those events, and only this site can apply them.
      const rejoined = recorded && applyBufferedChatEvents(recorded, buffered.get(recorded.id) ?? [])
      if (recorded) buffered.delete(recorded.id)
      const settled = !!rejoined && settledPhases.has(rejoined.phase)
      onHistoryStart()
      onAnnounce(null)
      onThreadSelected(threadId)
      onFreshThread(false)
      publishMessages(rejoined
        ? published.map((message) => message.run?.id === rejoined.id ? { ...message, run: rejoined } : message)
        : published)
      onActive(settled ? null : rejoined)
      if (rejoined) onAnnounce(rejoined)
      onHistoryLoaded()
      onFollow()
      onHistoryError('')
      switchBlocked = false
      // The buffer can hold the whole run, so a rejoin can settle at once. The
      // refresh stays unawaited because this call already bumped the refresh
      // sequence. An awaited refresh would reorder onThreadSelected.
      if (settled) void refreshThreads()
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
        onHistoryError('Conversation history could not be restored.', { label: 'Restore history', run: loadHistory })
      }
    } finally {
      loadingHistory = false
      releaseBuffer()
      switchingThread = false
      if (!destroyed) onThreadSwitch(switchBlocked)
    }
  }

  async function newThread() {
    if (active() || switchingThread || switchBlocked) return
    threadRefreshSequence += 1
    switchingThread = true
    onThreadSwitch(true)
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
      onHistoryError('')
    } catch (_) {
      if (!destroyed) onHistoryError('A new thread could not be started.', { label: 'Start new thread', run: newThread })
    } finally {
      switchingThread = false
      if (!destroyed) onThreadSwitch(switchBlocked)
    }
  }

  async function renameThread(title, previousTitle, threadId = readThreadId()) {
    const trimmed = title.trim()
    if (!threadId || !trimmed || trimmed === previousTitle) return false
    const previousRename = renameQueues.get(threadId) ?? Promise.resolve()
    const rename = previousRename.then(async () => {
      if (destroyed) return false
      try {
        await invoke('chat_rename_thread', { threadId, title: trimmed })
        if (destroyed) return false
        onThreadSummaries(readThreadSummaries().map((summary) => (
          summary.threadId === threadId ? { ...summary, title: trimmed } : summary
        )))
        onHistoryError('')
        return true
      } catch (_) {
        if (!destroyed) {
          onHistoryError('The thread name could not be changed.', {
            label: 'Rename thread again',
            run: () => renameThread(trimmed, previousTitle, threadId),
          })
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
      onHistoryError('')
      return true
    } catch (_) {
      if (!destroyed) onHistoryError('The thread could not be deleted.', { label: 'Delete thread', run: () => deleteThread(threadId) })
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
    holdBuffer()
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
    } finally {
      releaseBuffer()
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
    holdBuffer()
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
      const interrupted = { ...run, phase: 'interrupted', resumeError: typeof error === 'string' ? error : 'This reply could not be resumed.' }
      publishMessages(messages().map((message) => message.run?.id === run.id ? { ...message, run: interrupted } : message))
      onAnnounce(interrupted)
      onActive(null)
    } finally {
      releaseBuffer()
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

  return { start, loadHistory, loadOlderThreads, openThread: (threadId) => openThread(threadId, true), newThread, renameThread, deleteThread, send, cancel, resume, queue, cleanup }
}
