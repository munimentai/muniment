import { applyBufferedChatEvents, applyChatEvent, historyMessages, settledPhases, unsettledRun } from './chat-state.js'

const historyPageCap = 100
const historyPageLimit = 100
const signaledThreadCap = 256
const signaledRunCap = 256

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
  // How many calls replace the transcript with freshly read pages right now: a
  // thread open, an in-place re-read, or both at once. handleEvent buffers every
  // event while one runs, because no published run describes it yet.
  let historyLoads = 0
  let refreshingOpenThread = false
  let threadRefreshSequence = 0
  let threadPageCount = 1
  let nextThreadCursor = null
  let loadingOlderThreads = false
  // The shell shows a fresh thread and the first load after sign-in the same way:
  // no thread id. loadHistory reads this flag back to tell the two apart.
  let freshThread = false
  const renameQueues = new Map()
  // The signaled-thread set marks each new thread id once. A later event with
  // the same thread id does not refresh. The set is insertion-ordered and holds
  // at most 256 thread ids. It drops the oldest entry past that bound.
  const signaledThreads = new Set()
  let signaledThreadRefreshInFlight = false
  let signaledThreadRefreshFollowUp = false
  // The signaled-run set marks each new run id once. A later event with the
  // same run id does not re-read. The set is insertion-ordered and holds at
  // most 256 run ids. It drops the oldest entry past that bound.
  const signaledRuns = new Set()
  let signaledRunRefreshInFlight = false
  let signaledRunRefreshFollowUp = false

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
  const publishFreshThread = (next) => {
    freshThread = next
    onFreshThread(next)
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
        publishFreshThread(false)
      }
    } catch (_) {
      // A settlement refresh must not disturb the visible conversation state.
    }
  }

  // Signals that arrive during a refresh collapse into one follow-up refresh.
  async function refreshSignaledThreads() {
    signaledThreadRefreshInFlight = true
    try {
      do {
        signaledThreadRefreshFollowUp = false
        await refreshThreads()
      } while (signaledThreadRefreshFollowUp && !destroyed)
    } finally {
      signaledThreadRefreshInFlight = false
    }
  }

  function signalThread(threadId) {
    if (!threadId || destroyed) return false
    if (readThreadSummaries().some((summary) => summary.threadId === threadId)) return false
    if (signaledThreads.has(threadId)) return false
    if (signaledThreads.size >= signaledThreadCap) {
      signaledThreads.delete(signaledThreads.values().next().value)
    }
    signaledThreads.add(threadId)
    if (signaledThreadRefreshInFlight) {
      signaledThreadRefreshFollowUp = true
      return true
    }
    void refreshSignaledThreads()
    return true
  }

  // Signals that arrive during a re-read collapse into one follow-up re-read.
  async function refreshSignaledOpenThread() {
    signaledRunRefreshInFlight = true
    try {
      do {
        signaledRunRefreshFollowUp = false
        await refreshOpenThread()
      } while (signaledRunRefreshFollowUp && !destroyed)
    } finally {
      signaledRunRefreshInFlight = false
    }
  }

  function signalRun(runId, threadId) {
    if (!runId || !threadId || destroyed) return false
    if (threadId !== readThreadId()) return false
    if (messages().some((message) => message.run?.id === runId)) return false
    if (signaledRuns.has(runId)) return false
    if (signaledRuns.size >= signaledRunCap) {
      signaledRuns.delete(signaledRuns.values().next().value)
    }
    signaledRuns.add(runId)
    if (signaledRunRefreshInFlight || refreshingOpenThread) {
      signaledRunRefreshFollowUp = true
      return true
    }
    void refreshSignaledOpenThread()
    return true
  }

  function handleEvent({ payload }) {
    const signaled = signalThread(payload.threadId)
    // A thread load replaces the whole transcript, so no published run describes
    // this event yet. openThread and refreshOpenThread drain the buffer onto the
    // loaded pages.
    const current = historyLoads ? null : messages().find((message) => message.run?.id === payload.runId)?.run
    if (!current) {
      // The runtime broadcasts every attach run, so most unknown run ids belong
      // to another surface. Hold the event only while a call waits for its run
      // id. Drop it otherwise, or the map grows for the life of the window.
      if (runIdWaits) buffered.set(payload.runId, [...(buffered.get(payload.runId) ?? []), payload])
      // A new run on the open thread is the exception. Re-read those pages once.
      // Signal after the drop so this event is not applied twice onto the pages.
      signalRun(payload.runId, payload.threadId)
      return
    }
    const projected = applyChatEvent(current, payload)
    if (projected) publishMessages(messages().map((message) => message.run?.id === projected.id ? { ...message, run: projected } : message))
    if (projected && (!settledPhases.has(current.phase) || readAnnounced()?.id === payload.runId)) onAnnounce(projected)
    if (active()?.id === payload.runId) {
      const settled = projected && settledPhases.has(projected.phase)
      onActive(settled ? null : projected)
      if (settled && !signaled) void refreshThreads()
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
        publishFreshThread(true)
        onHistoryLoaded()
        onFollow()
        return
      }
      const openThreadId = readThreadId()
      // A reconnect reloads the history, so the thread the user has open must
      // stay open. Only the first load after sign-in falls through to the newest
      // thread. A blocked switch falls through too, because openThread alone
      // restores the backend selection.
      if (!openThreadId && freshThread && !switchBlocked) {
        onHistoryError('')
        return
      }
      const newest = summaries[0].threadId
      if (openThreadId && openThreadId !== newest) {
        // Retention can prune the open thread, and a sign-in under another
        // subject leaves the shell holding an id it no longer owns. Fall back to
        // the newest thread, or every reconnect retries the same dead id.
        if (await openThread(openThreadId, switchBlocked) !== false) return
        if (destroyed) return
        await openThread(newest, switchBlocked)
        return
      }
      await openThread(newest, switchBlocked)
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
    historyLoads += 1
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
      publishFreshThread(false)
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
      // loadHistory reads this back: false means the thread did not load, so the
      // caller can fall back. A skipped call returns undefined instead.
      return true
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
      return false
    } finally {
      historyLoads -= 1
      releaseBuffer()
      switchingThread = false
      if (!destroyed) onThreadSwitch(switchBlocked)
    }
  }

  // A re-read that publishes no pages still holds every event that landed while
  // those pages loaded, because handleEvent buffered them all. releaseBuffer
  // drops them next, so hand them to the run the shell shows first.
  function drainVisibleRun() {
    const run = destroyed ? null : active()
    const held = run && buffered.get(run.id)
    if (!held?.length) return
    const projected = applyBufferedChatEvents(run, held)
    buffered.delete(run.id)
    publishMessages(messages().map((message) => message.run?.id === run.id ? { ...message, run: projected } : message))
    onAnnounce(projected)
    const settled = settledPhases.has(projected.phase)
    onActive(settled ? null : projected)
    if (settled) void refreshThreads()
  }

  // The runtime drops a chat-event subscriber whose queue fills, and the desktop
  // resubscribes after a retry. The open run keeps whatever hole that gap left,
  // so this re-read repairs it in place. It reads the same pages openThread
  // reads, and it selects no thread, so a run in flight keeps running. It also
  // leaves a stale history error standing, because a background call owns no
  // part of the visible error state.
  async function refreshOpenThread() {
    const threadId = readThreadId()
    if (!threadId || destroyed || switchingThread || refreshingOpenThread) return
    // A submission still waiting for its run id owns the transcript tail, and no
    // page can carry that run yet. A re-read would drop it.
    if (active()?.id === 'pending') return
    refreshingOpenThread = true
    // send, resume, queue and a concurrent openThread each publish a transcript
    // this call cannot see. Any of them replaces this array, and the pages go
    // stale the moment that happens.
    const publishedAtEntry = messages()
    historyLoads += 1
    holdBuffer()
    try {
      const history = []
      let cursor
      for (let page = 0; page < historyPageCap; page += 1) {
        const payload = { threadId, limit: historyPageLimit }
        if (cursor !== undefined) payload.cursor = cursor
        const result = await invoke('chat_thread_open', payload)
        // A thread switch can land while the pages load. The pages then describe
        // a thread the shell no longer shows, so this call drops them.
        if (destroyed || readThreadId() !== threadId) return
        history.push(...result.entries)
        if (result.nextCursor == null) break
        cursor = result.nextCursor
      }
      // Another call published while the pages loaded, so the pages describe an
      // older transcript. That call owns the view now, and publishing the pages
      // would drop the message and the run it just added.
      if (messages() !== publishedAtEntry) {
        drainVisibleRun()
        return
      }
      const published = historyMessages(history)
      const recorded = unsettledRun(published)
      // handleEvent buffered every event that landed during the re-read, and the
      // pages are stale by exactly those events. Only this site can apply them.
      const rejoined = recorded && applyBufferedChatEvents(recorded, buffered.get(recorded.id) ?? [])
      if (recorded) buffered.delete(recorded.id)
      const settled = !!rejoined && settledPhases.has(rejoined.phase)
      const republished = rejoined
        ? published.map((message) => message.run?.id === rejoined.id ? { ...message, run: rejoined } : message)
        : published
      publishMessages(republished)
      // The pages carry the run the runtime still drives, so the shell keeps
      // that run active. A re-read that finds no such run ends the active one,
      // because the settlement is the event the dropped subscriber lost.
      onActive(settled ? null : rejoined)
      // The live region follows one run. Restate that run alone, so a re-read
      // never switches the subject the user hears.
      const announcedId = readAnnounced()?.id
      const tracked = announcedId && republished.find((message) => message.run?.id === announcedId)?.run
      if (tracked) onAnnounce(tracked)
      if (settled) void refreshThreads()
    } catch (_) {
      // A background re-read must not disturb the visible conversation state.
      // This call repairs a hole a dropped subscriber left, so a failed read
      // must not open a wider one.
      drainVisibleRun()
    } finally {
      historyLoads -= 1
      releaseBuffer()
      refreshingOpenThread = false
    }
    // A signal can land on a re-read that a reconnect started. Collapse it
    // into one follow-up once that re-read ends.
    if (signaledRunRefreshFollowUp && !signaledRunRefreshInFlight && !destroyed) {
      void refreshSignaledOpenThread()
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
      publishFreshThread(true)
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
        publishFreshThread(true)
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

  return { start, loadHistory, loadOlderThreads, openThread: (threadId) => openThread(threadId, true), refreshOpenThread, newThread, renameThread, deleteThread, send, cancel, resume, queue, cleanup }
}
