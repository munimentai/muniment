import { applyChatEvent, historyMessages, settledPhases, unsettledRun, withFolders, withSteers } from './chat-state.js'
import { LOCAL_RUNTIME_ID } from '../runtimes/runtimes.js'

const historyPageCap = 100
const historyPageLimit = 100
const signaledThreadCap = 256
const signaledRunCap = 256
const liveTextCap = 256
const desktopBusy = 'Muniment is busy with another request. Try again.'
const desktopBusyStart = 'Muniment is busy with another request.'
// The waits before each select retry, in milliseconds.
const selectRetryDelays = [250, 750, 1500]

// The notice a refused select shows over the thread it was for.
function selectFailureText(error) {
  const cause = typeof error === 'string' ? error : error?.message
  const failure = 'This thread did not open, so sending waits.'
  return typeof cause === 'string' && cause.trim() ? `${failure} ${cause}` : failure
}
// The runtime admits one active run per profile and answers a second submit
// with this reason before it records anything (attach_boundaries.rs).
const replyInProgress = 'A reply is already in progress.'
const replyInProgressRetryMs = 2_000
const runElsewhere = 'Another reply is still running. Send again when it ends.'
// The runtime refuses a run in a thread another fleet member runs with this
// reason before it records anything (attach_boundaries.rs).
const threadElsewhere = 'Another machine runs this thread. Send the message to that machine.'
const terminalTypes = { completed: 'complete', cancelled: 'cancelled', failed: 'failed' }
const eventSettles = (event) => settledPhases.has(event.phase ?? terminalTypes[event.type])
const submissionSources = ['local-mode', 'auth', 'thread', 'submit']

function submissionFailure(error) {
  const structured = error && submissionSources.includes(error.source) && typeof error.message === 'string'
  return { source: structured ? error.source : 'unknown', message: structured ? error.message : error }
}

function historyReadError(error) {
  const cause = typeof error === 'string' ? error : error?.message
  const failure = 'Muniment could not restore conversation history.'
  return typeof cause === 'string' && cause.trim() ? `${failure} ${cause}` : failure
}

export function createChatController({
  invoke,
  listen,
  readMessages,
  readActive,
  readAnnounced,
  readDraft,
  readFiles,
  // The composer's own draft and files, which a refused send puts back.
  readComposer = () => ({ text: readDraft(), files: readFiles() }),
  // The thinking level a send carries, or null for the model's default.
  readThinkingLevel = () => null,
  // The permission mode picked for a new thread before its first message, or
  // null. Core sets it before the run starts, so it covers the first tool call.
  readPermissionMode = () => null,
  // The model the composer picked before any thread exists. The first
  // message carries it to the thread it starts.
  readPendingModel = () => null,
  // The ids of the agents a prompt mentions. Core sends the prompt to each
  // and answers what became of each one, which `onMentions` receives.
  readMentions = () => [],
  onMentions = () => {},
  readThreadId = () => null,
  // The thread open when the app last quit, which launch opens again while it
  // still exists, or null.
  readLaunchThreadId = () => null,
  readThreadSummaries = () => [],
  // The messages sent during each reply, by run id, which the window keeps,
  // and the call that keeps one more.
  readSteers = () => ({}),
  onSteer = () => {},
  // Whether the user archived a thread. Archive state lives in the window,
  // so the first page reads on until it holds a full page of other threads.
  readArchived = () => false,
  // The machine the composer sends to. A run left running on one machine
  // does not hold a send to another.
  readRuntime = () => null,
  blocked = () => false,
  onMessages,
  onActive,
  onAnnounce,
  onDraft,
  onFiles,
  onSubmitError,
  onCancelError,
  onQueueError,
  onHistoryError: publishHistoryError,
  onHistoryStart = () => {},
  onThreadSummaries = () => {},
  // The threads the local runtime names as deleted, from each first page.
  onDeletedThreads = () => {},
  // A deleted thread whose folder stays because it holds the user's files:
  // the thread's summary and core's answer, which names the folder.
  onThreadFolderKept = () => {},
  onMoreThreads = () => {},
  onThreadListError = () => {},
  readProject = () => null,
  readAgent = () => null,
  onThreadSelected = () => {},
  onThreadSwitch = () => {},
  onFreshThread = () => {},
  onHistoryLoaded = () => {},
  onFollow = () => {},
  onFocus = () => {},
  onSend = () => {},
  prepareSubmit = () => {},
  onSubmitted = () => {},
  onSubmission = () => {},
  onSignInLink = () => {},
  onBackgroundRun = () => {},
  // The messages posted to a thread on swarm members, from its first page.
  onPosted = () => {},
  // Sends a message the runtime refused because another machine runs the
  // thread to that machine, and answers whether it went.
  postElsewhere = null,
  // How long launch waits for this computer's runtime to answer the thread
  // list, and the pause between asks.
  localListWaitMs = 10_000,
  localListRetryMs = 250,
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
  let historyReadFailed = false
  // The whole-history read failed, so a reconnect repeats it instead of a page refresh.
  let historyLoadFailed = false
  let destroyed = false
  let switchingThread = false
  // The run the window left on another thread, as { threadId, runId }. The
  // runtime keeps driving it, and its settling event clears it.
  let background = null
  let switchBlocked = false
  // How many calls replace the transcript with freshly read pages right now: a
  // thread open, an in-place re-read, or both at once. handleEvent buffers every
  // event while one runs, because no published run describes it yet.
  let historyLoads = 0
  let refreshingOpenThread = false
  let recoveryPending = false
  let recovering = false
  let recoveryRetry
  let deliveryFailureReason = ''
  let threadRefreshSequence = 0
  let threadPageCount = 1
  let nextThreadCursor = null
  // Whether this computer's list ended: a page it answered named no next
  // cursor. Until then a refresh's cursor restores the next page.
  let threadListEnded = false
  // The read of the next page in flight. A second caller shares it, so a
  // search, the Older threads control and the fill never race for a page.
  let olderThreadsLoad = null
  // Each history load starts the list again, so a page read for the list
  // it replaced is read again from the new list's cursor.
  let threadListSequence = 0
  // Whether the last page read failed. The list then waits for the user's
  // Retry, and the fill stops.
  let olderThreadsFailed = false
  const threadPageSize = 20
  // The shell shows a fresh thread and the first load after sign-in the same way:
  // no thread id. loadHistory reads this flag back to tell the two apart.
  let freshThread = false
  const renameQueues = new Map()
  // The signaled-thread set marks each new thread id once. A later event with
  // the same thread id does not refresh. The set is insertion-ordered and holds
  // at most 256 thread ids. It drops the oldest entry past that bound.
  const signaledThreads = new Set()
  // The machine each select named, by thread id.
  const selectedRuntimes = new Map()
  // Whether this computer's runtime answered the last thread list. A list it
  // did not answer holds the paired machines' synced copies of its threads.
  let localListed = true
  // Whether a thread list published yet, and whether launch waits for this
  // computer's list now. A refresh during that wait publishes nothing.
  let launchListed = false
  let launchWaiting = false
  const listedLocally = (page) => !Array.isArray(page?.runtimeIds) || page.runtimeIds.includes(LOCAL_RUNTIME_ID)
  let signaledThreadRefreshInFlight = false
  let signaledThreadRefreshFollowUp = false
  // The signaled-run set marks each new run id once. A later event with the
  // same run id does not re-read. The set is insertion-ordered and holds at
  // most 256 run ids. It drops the oldest entry past that bound.
  const signaledRuns = new Set()
  let signaledRunRefreshInFlight = false
  let signaledRunRefreshFollowUp = false
  // The runtime streams text as deltas: the appended text and textStart, its
  // UTF-16 offset in the run's text. A delta extends only the text that the
  // last live event of its run left, so this map holds that text by run id.
  // The runtime sends a whole state only when the run state changes, so a run
  // restored from pages starts its entry from the page text. Every event that
  // landed while the pages loaded waits in the buffer, and the journal holds a
  // delta before the runtime sends it, so the buffered deltas extend that text.
  const liveText = new Map()
  // The run-index map finds a streamed run without a scan. A lookup checks the
  // index against the transcript and rebuilds the map when the layout changed.
  let indexedMessages = null
  let runIndexes = new Map()

  function onHistoryError(message, action, isHistoryRead = false) {
    historyReadFailed = isHistoryRead
    if (action) publishHistoryError(message, action)
    else publishHistoryError(message)
  }

  function holdBuffer() {
    runIdWaits += 1
  }

  function releaseBuffer() {
    runIdWaits -= 1
    if (!runIdWaits) buffered.clear()
  }

  const messages = () => readMessages()

  function runIndex(runId) {
    const list = messages()
    const index = runIndexes.get(runId)
    if (index !== undefined && list[index]?.run?.id === runId) return index
    if (index === undefined && list === indexedMessages) return -1
    runIndexes = new Map()
    list.forEach((message, position) => {
      if (message.run?.id != null) runIndexes.set(message.run.id, position)
    })
    indexedMessages = list
    return runIndexes.get(runId) ?? -1
  }

  // Publishes a copy of the transcript with one run replaced. The shell holds
  // the transcript as raw state, so every change is a new array and new objects.
  function replaceRun(index, run) {
    const next = messages().slice()
    next[index] = { ...next[index], run }
    publishMessages(next)
    // The layout is unchanged, so the map stays valid for the new array.
    if (runIndexes.get(run.id) === index) indexedMessages = messages()
  }

  function rememberLiveText(run) {
    if (settledPhases.has(run.phase)) {
      liveText.delete(run.id)
      return
    }
    liveText.delete(run.id)
    if (liveText.size >= liveTextCap) liveText.delete(liveText.keys().next().value)
    liveText.set(run.id, run.text ?? '')
  }

  // Expands a text delta into the whole event it stands for. It returns null
  // when the run's text is not the text the delta extends.
  function wholeEvent(run, event) {
    if (event.textStart == null) return event
    const base = liveText.get(event.runId)
    if (base === undefined || base !== run.text || event.textStart > base.length) return null
    const text = event.textStart === base.length ? base + event.text : base.slice(0, event.textStart) + event.text
    return {
      runId: event.runId,
      threadId: event.threadId,
      phase: event.phase,
      text,
      promptStorageNotice: run.promptStorageNotice,
      resumeNotice: run.resumeNotice,
      turnStarted: run.turnStarted,
      failureReason: run.failureReason,
      failureDetail: run.failureDetail,
      receipt: run.receipt,
      recalls: run.recalls,
      toolActivity: run.toolActivity,
      attachments: run.attachments,
      appliedDiffs: run.appliedDiffs,
      pendingPermission: run.pendingPermission,
    }
  }

  // Applies one live event, a whole state or a delta, and returns the run
  // unchanged when a delta does not extend it.
  function applyLiveEvent(run, event) {
    if (!run || event.runId !== run.id) return run
    // A queued snapshot cannot reopen a terminal run restored from the journal.
    // Interrupted runs can resume, so they do not use this guard.
    if (['complete', 'cancelled', 'failed'].includes(run.phase)) {
      const phase = event.phase ?? terminalTypes[event.type]
      if (phase !== run.phase) return run
    }
    const whole = wholeEvent(run, event)
    if (!whole) return run
    const projected = applyChatEvent(run, whole)
    if (projected !== run) rememberLiveText(projected)
    return projected
  }

  const applyLiveEvents = (run, events) => events.reduce(applyLiveEvent, run)
  // Rejoins the unsettled run a page load restored, with the events that
  // landed while the pages loaded.
  function rejoinRun(run) {
    rememberLiveText(run)
    return applyLiveEvents(run, buffered.get(run.id) ?? [])
  }
  const active = () => readActive()
  // A submission waiting for its run id, a resume, or a recovery owns the
  // transcript tail, so the window stays on its thread until it ends.
  const holdsThread = () => {
    const run = active()
    return !!run && (run.id === 'pending' || run.phase === 'resuming' || run.phase === 'recovering')
  }
  const publishBackground = (next) => {
    background = next
    if (!destroyed) onBackgroundRun(next)
  }

  // The window leaves its active run for another thread. A run whose settling
  // event already arrived ends here, and any other run stays in the background.
  function leaveRun(run, threadId, held = []) {
    if (!run) return
    if (held.some(eventSettles)) {
      void refreshThreads()
      return
    }
    const runtime = readThreadSummaries().find((summary) => summary.threadId === threadId)?.runtimeId ?? readRuntime() ?? LOCAL_RUNTIME_ID
    publishBackground({ threadId: threadId ?? null, runId: run.id, runtime })
  }
  // Whether the run left in the background runs on the machine the composer
  // sends to, so a send there waits for it.
  const backgroundHere = () => !!background && (background.runtime ?? LOCAL_RUNTIME_ID) === (readRuntime() ?? LOCAL_RUNTIME_ID)
  const publishMessages = (next) => {
    if (!destroyed) onMessages(next)
  }
  const publishFreshThread = (next) => {
    freshThread = next
    onFreshThread(next)
  }

  async function refreshThreads(reportFailure = false) {
    const sequence = ++threadRefreshSequence
    try {
      const [firstPage, currentThreadId] = await Promise.all([
        invoke('chat_thread_summaries', { limit: 20 }),
        invoke('chat_current_thread'),
      ])
      if (destroyed || sequence !== threadRefreshSequence || launchWaiting) return false
      onDeletedThreads(firstPage.deletedThreadIds ?? [])
      localListed = listedLocally(firstPage)
      const refreshedThreadIds = new Set(firstPage.summaries.map((summary) => summary.threadId))
      // A machine that did not answer, such as one that revoked this device, leaves its threads out.
      const answered = Array.isArray(firstPage.runtimeIds) ? new Set(firstPage.runtimeIds) : null
      const previous = readThreadSummaries()
      const retainedSummaries = previous.filter((summary) => !refreshedThreadIds.has(summary.threadId)
        && (!answered || !summary.runtimeId || answered.has(summary.runtimeId)))
      // A synced copy never stands in for a thread its own machine listed
      // before and did not answer for this time, so its row and the title's
      // machine stay put while that machine is busy.
      const held = new Map(answered ? previous.filter((summary) => summary.runtimeId && !answered.has(summary.runtimeId)).map((summary) => [summary.threadId, summary]) : [])
      const listed = firstPage.summaries.map((summary) => summary.replica && held.has(summary.threadId) ? held.get(summary.threadId) : summary)
      const summaries = [...listed, ...retainedSummaries]
      onThreadSummaries(summaries)
      // A list whose first read failed or held no next cursor, such as one a
      // paired machine answered alone, takes the next page from this read.
      if (nextThreadCursor == null && !threadListEnded && firstPage.nextCursor != null && localListed) {
        nextThreadCursor = firstPage.nextCursor
        onMoreThreads(true)
      }
      void fillThreads()
      const shownThreadId = currentThreadId ?? (freshThread ? null : readThreadId())
      onThreadSelected(shownThreadId)
      if (shownThreadId && summaries.some((summary) => summary.threadId === shownThreadId)) {
        publishFreshThread(false)
      }
      return true
    } catch (error) {
      if (reportFailure) throw error
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
    if (runIndex(runId) >= 0) return false
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
    if (destroyed) return
    // The runtime announces the sign-in link on this stream. It names no run.
    if (payload.phase === 'sign-in-link') {
      onSignInLink(payload.text)
      return
    }
    if (payload.phase === 'delivery-failed') {
      deliveryFailureReason = payload.failureReason || 'Reply delivery failed.'
      onHistoryError(deliveryFailureReason, { label: 'Restore reply', run: recoverChatEvents })
      void recoverChatEvents()
      return
    }
    const signaled = signalThread(payload.threadId)
    if (background && payload.runId === background.runId) {
      if (eventSettles(payload)) {
        publishBackground(null)
        if (!signaled) void refreshThreads()
      } else if (!background.threadId && payload.threadId) {
        publishBackground({ ...background, threadId: payload.threadId })
      }
    }
    // A thread load replaces the whole transcript, so no published run describes
    // this event yet. openThread and refreshOpenThread drain the buffer onto the
    // loaded pages.
    const index = historyLoads ? -1 : runIndex(payload.runId)
    const current = index < 0 ? null : messages()[index].run
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
    if (payload.turnStarted && !current.turnStarted) void refreshThreads()
    const projected = applyLiveEvent(current, payload)
    // A delta that does not extend the shown text waits for the next whole state.
    if (projected === current) return
    if (projected) replaceRun(index, projected)
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
      // At launch a paired machine can answer before this computer's
      // runtime. Its list holds synced copies of this computer's threads,
      // which would name the paired machine on each row, in the title and in
      // the restored thread's select. Launch asks again until this computer
      // answers or the wait ends, and the shell keeps its loading state.
      launchWaiting = !launchListed
      let page
      try {
        page = await invoke('chat_thread_summaries', { limit: 20 })
        const deadline = Date.now() + localListWaitMs
        while (launchWaiting && !listedLocally(page) && Date.now() < deadline && !destroyed) {
          await new Promise((resolve) => setTimeout(resolve, localListRetryMs))
          if (destroyed) return
          try { page = await invoke('chat_thread_summaries', { limit: 20 }) } catch (_) { /* The list in hand stands. */ }
        }
      } finally {
        launchWaiting = false
      }
      const { summaries, nextCursor, deletedThreadIds } = page
      if (destroyed) return
      localListed = listedLocally(page)
      launchListed = true
      onDeletedThreads(deletedThreadIds ?? [])
      historyLoadFailed = false
      threadPageCount = 1
      threadListSequence += 1
      nextThreadCursor = nextCursor
      threadListEnded = nextCursor == null && localListed
      onThreadSummaries(summaries)
      onMoreThreads(nextThreadCursor != null)
      await fillThreads()
      if (destroyed) return
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
      // A reload keeps the run the user watches on screen.
      if (active()) return
      const newest = summaries[0].threadId
      // At launch the shell holds no thread yet. The thread open at quit opens
      // again, and one that is gone falls through to the newest with no error.
      const launchThreadId = openThreadId ? null : readLaunchThreadId()
      if (launchThreadId && launchThreadId !== newest) {
        if (await openThread(launchThreadId, switchBlocked, !switchBlocked, true) !== false) return
        if (destroyed) return
      }
      if (openThreadId && openThreadId !== newest) {
        // Retention can prune the open thread, and a sign-in under another
        // subject leaves the shell holding an id it no longer owns. Fall back to
        // the newest thread, or every reconnect retries the same dead id.
        if (await openThread(openThreadId, switchBlocked) !== false) return
        if (destroyed) return
        await openThread(newest, true)
        return
      }
      // A new desktop has no backend selection. Restore it with the transcript.
      await openThread(newest, switchBlocked, !openThreadId && !switchBlocked)
    } catch (error) {
      if (!destroyed) {
        historyLoadFailed = true
        onHistoryError(historyReadError(error), { label: 'Restore history', run: loadHistory }, true)
      }
    }
  }

  // Archived threads count toward a page, so the list reads older pages
  // until it holds a page of threads the user did not archive or ends. A
  // page read in flight is awaited, never a reason to stop.
  async function fillThreads() {
    for (let pages = 0; pages < 50; pages += 1) {
      if (destroyed || nextThreadCursor == null) return
      const listed = readThreadSummaries()
      const shown = listed.filter((summary) => !readArchived(summary.threadId)).length
      if (shown >= threadPageSize || shown === listed.length) return
      await loadOlderThreads()
      if (olderThreadsFailed) return
    }
  }

  function loadOlderThreads() {
    if (destroyed || nextThreadCursor == null) return Promise.resolve(null)
    olderThreadsLoad ??= readOlderThreads()
    return olderThreadsLoad
  }

  async function readOlderThreads() {
    // A history load that replaced the list during the read leaves its page
    // unused, and the read goes on from the new list's cursor.
    try {
      for (let attempt = 0; attempt < 3; attempt += 1) {
        if (destroyed || nextThreadCursor == null) return null
        const cursor = nextThreadCursor
        const sequence = threadListSequence
        let result
        try {
          result = await invoke('chat_thread_summaries', { limit: threadPageSize, cursor })
        } catch (_) {
          if (destroyed) return null
          if (sequence !== threadListSequence) continue
          olderThreadsFailed = true
          onThreadListError('Older threads could not be loaded.', { label: 'Load older threads', run: loadOlderThreads })
          return null
        }
        if (destroyed) return null
        if (sequence !== threadListSequence || cursor !== nextThreadCursor) continue
        olderThreadsFailed = false
        const firstThreadId = result.summaries[0]?.threadId ?? null
        onThreadSummaries([...readThreadSummaries(), ...result.summaries])
        threadPageCount += 1
        nextThreadCursor = result.nextCursor
        if (result.nextCursor == null && listedLocally(result)) threadListEnded = true
        onMoreThreads(nextThreadCursor != null)
        onThreadListError('')
        return firstThreadId
      }
      return null
    } finally {
      olderThreadsLoad = null
    }
  }

  // A quiet open reports no failure, since its caller falls back to another
  // thread or names the failure itself through `onFailure`.
  async function openThread(threadId, select = false, restoreSelection = false, quiet = false, onFailure = null) {
    if (switchingThread || holdsThread()) return
    threadRefreshSequence += 1
    switchingThread = true
    onThreadSwitch(true)
    const previousThreadId = readThreadId()
    // Each select names its thread's machine as the list shows it now, so a
    // list that changes during the load sends no select to another machine.
    const target = selection(threadId)
    const previous = previousThreadId == null ? null : selection(previousThreadId)
    const wasBlocked = switchBlocked
    let selected = false
    // A select the runtime refused: the thread still shows, and sending waits
    // until a select of it succeeds, so no message goes to the thread before.
    let selectFailure = null
    historyLoads += 1
    holdBuffer()
    try {
      if (select) {
        try {
          await selectThread(target)
          selected = true
        } catch (error) {
          // A select of the thread already shown leaves nothing to mix up.
          if (quiet || destroyed || threadId === previousThreadId) throw error
          selectFailure = error
        }
      }
      const history = []
      let posted = []
      let cursor
      for (let page = 0; page < historyPageCap; page += 1) {
        const payload = { threadId, limit: historyPageLimit }
        if (cursor !== undefined) payload.cursor = cursor
        const result = await invoke('chat_thread_open', payload)
        if (destroyed) return
        if (page === 0) posted = result.posted ?? []
        history.push(...result.entries)
        if (result.nextCursor == null) break
        cursor = result.nextCursor
      }
      if (destroyed) return
      // Read first so a failed initial history read still permits a fresh thread.
      if (restoreSelection && !selected && !selectFailure) {
        await invoke('chat_select_thread', target)
        selected = true
        if (destroyed) return
      }
      const published = withSteers(historyMessages(history), readSteers())
      // The runtime keeps driving an unsettled run while the window is away. The
      // desktop rejoins that run instead of starting one. handleEvent carries it
      // forward by run id from here.
      const recorded = unsettledRun(published)
      // handleEvent buffered every event that landed during the load, so the
      // pages loaded over a growing buffer. The recorded state is stale by
      // exactly those events, and only this site can apply them.
      const rejoined = recorded && rejoinRun(recorded)
      if (recorded) buffered.delete(recorded.id)
      const settled = !!rejoined && settledPhases.has(rejoined.phase)
      const leaving = threadId !== previousThreadId ? active() : null
      if (leaving) leaveRun(leaving, previousThreadId, buffered.get(leaving.id))
      else if (background && (threadId === background.threadId || rejoined?.id === background.runId)) publishBackground(null)
      onHistoryStart()
      onAnnounce(null)
      onThreadSelected(threadId)
      publishFreshThread(false)
      publishMessages(rejoined
        ? published.map((message) => message.run?.id === rejoined.id ? { ...message, run: rejoined } : message)
        : published)
      onPosted(threadId, posted)
      onActive(settled ? null : rejoined)
      if (rejoined) onAnnounce(rejoined)
      onHistoryLoaded()
      onFollow()
      if (selectFailure) {
        switchBlocked = true
        onHistoryError(selectFailureText(selectFailure), { label: 'Open again', run: () => openThread(threadId, true) })
      } else {
        onHistoryError('')
        switchBlocked = false
      }
      // The buffer can hold the whole run, so a rejoin can settle at once. The
      // refresh stays unawaited because this call already bumped the refresh
      // sequence. An awaited refresh would reorder onThreadSelected.
      if (settled) void refreshThreads()
      // loadHistory reads this back: false means the thread did not load, so the
      // caller can fall back. A skipped call returns undefined instead.
      return true
    } catch (error) {
      if (!destroyed) {
        if (wasBlocked) {
          switchBlocked = true
        } else if (selected) {
          if (previousThreadId == null) {
            switchBlocked = true
          } else {
            try {
              await invoke('chat_select_thread', previous)
              switchBlocked = false
            } catch (_) {
              switchBlocked = true
            }
          }
        }
        // The transcript stays on the run the user left, so it takes the
        // events that landed while the other thread loaded.
        drainVisibleRun()
        if (quiet) onFailure?.(error)
        if (!quiet) onHistoryError(historyReadError(error), {
          label: 'Restore history',
          run: () => openThread(threadId, select || switchBlocked, restoreSelection),
        }, true)
      }
      return false
    } finally {
      historyLoads -= 1
      releaseBuffer()
      switchingThread = false
      if (!destroyed) onThreadSwitch(switchBlocked)
      if (recoveryPending) void recoverChatEvents()
    }
  }

  // A busy runtime answers a select in a moment, so a select tries again a
  // few times before it reports the refusal.
  async function selectThread(target) {
    for (let attempt = 0; ; attempt += 1) {
      try {
        return await invoke('chat_select_thread', target)
      } catch (error) {
        const cause = typeof error === 'string' ? error : error?.message
        if (destroyed || attempt >= selectRetryDelays.length || !String(cause ?? '').startsWith(desktopBusyStart)) throw error
        await new Promise((resolve) => setTimeout(resolve, selectRetryDelays[attempt]))
      }
    }
  }

  // A re-read that publishes no pages still holds every event that landed while
  // those pages loaded, because handleEvent buffered them all. releaseBuffer
  // drops them next, so hand them to the run the shell shows first.
  function drainVisibleRun() {
    const run = destroyed ? null : active()
    const held = run && buffered.get(run.id)
    if (!held?.length) return
    const projected = applyLiveEvents(run, held)
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
  // reads, and it selects no thread, so a run in flight keeps running.
  // A successful re-read clears history-read alerts but keeps unrelated action errors.
  async function refreshOpenThread(reportFailure = false) {
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
      let posted = []
      let cursor
      for (let page = 0; page < historyPageCap; page += 1) {
        const payload = { threadId, limit: historyPageLimit }
        if (cursor !== undefined) payload.cursor = cursor
        const result = await invoke('chat_thread_open', payload)
        if (page === 0) posted = result.posted ?? []
        // A thread switch can land while the pages load. The pages then describe
        // a thread the shell no longer shows, so this call drops them.
        if (destroyed || readThreadId() !== threadId) return
        history.push(...result.entries)
        if (result.nextCursor == null) break
        cursor = result.nextCursor
      }
      onPosted(threadId, posted)
      // Another call published while the pages loaded, so the pages describe an
      // older transcript. That call owns the view now, and publishing the pages
      // would drop the message and the run it just added.
      if (messages() !== publishedAtEntry) {
        if (recovering) recoveryPending = true
        drainVisibleRun()
        return
      }
      const published = withSteers(historyMessages(history), readSteers())
      const recorded = unsettledRun(published)
      // handleEvent buffered every event that landed during the re-read, and the
      // pages are stale by exactly those events. Only this site can apply them.
      const rejoined = recorded && rejoinRun(recorded)
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
      if (historyReadFailed) onHistoryError('')
      if (settled) void refreshThreads()
    } catch (error) {
      // A background re-read must not disturb the visible conversation state.
      // This call repairs a hole a dropped subscriber left, so a failed read
      // must not open a wider one.
      drainVisibleRun()
      if (reportFailure) throw error
    } finally {
      historyLoads -= 1
      releaseBuffer()
      refreshingOpenThread = false
      if (recoveryPending && !recovering) void recoverChatEvents()
    }
    // A signal can land on a re-read that a reconnect started. Collapse it
    // into one follow-up once that re-read ends.
    if (signaledRunRefreshFollowUp && !signaledRunRefreshInFlight && !destroyed) {
      void refreshSignaledOpenThread()
    }
    return true
  }

  async function recoverChatEvents() {
    if (destroyed) return
    recoveryPending = true
    if (recovering || active()?.id === 'pending' || switchingThread || refreshingOpenThread) return
    clearTimeout(recoveryRetry)
    recovering = true
    try {
      do {
        recoveryPending = false
        // A history read that failed before the runtime connected is the hole to
        // repair. Read the whole history again, and a failure retries below.
        if (historyLoadFailed) {
          await loadHistory()
          recoveryPending = historyLoadFailed
          break
        }
        // The first lost frame can name a new thread. Resolve it before reading the journal.
        if (!await refreshThreads(true) || active()?.id === 'pending' || switchingThread || refreshingOpenThread) {
          recoveryPending = true
          break
        }
        if (readThreadId() && !await refreshOpenThread(true)) {
          recoveryPending = true
          break
        }
      } while (recoveryPending && !destroyed && active()?.id !== 'pending' && !switchingThread)
      if (!recoveryPending) deliveryFailureReason = ''
    } catch (error) {
      recoveryPending = true
      if (!destroyed) onHistoryError(deliveryFailureReason || historyReadError(error), { label: 'Restore reply', run: recoverChatEvents }, !deliveryFailureReason)
    } finally {
      recovering = false
      // Request connectivity can recover without another chat subscription event.
      if (recoveryPending && !destroyed) recoveryRetry = setTimeout(recoverChatEvents, 1000)
    }
  }

  async function newThread(prompt) {
    const fromLauncher = typeof prompt === 'string'
    if (destroyed || switchingThread || switchBlocked || holdsThread() || (fromLauncher && (active() || background || blocked()))) return false
    threadRefreshSequence += 1
    switchingThread = true
    onThreadSwitch(true)
    const previousThreadId = readThreadId()
    try {
      const projectId = fromLauncher ? null : readProject()
      const agentId = fromLauncher ? null : readAgent()
      const context = { ...(projectId ? { projectId } : {}), ...(agentId ? { agentId } : {}) }
      const createdThread = await invoke('chat_new_thread', ...(Object.keys(context).length ? [context] : []))
      if (destroyed) return false
      // Events kept applying to the open run while the thread was created, so
      // a run still active here is one the runtime still drives.
      leaveRun(active(), previousThreadId)
      onActive(null)
      onHistoryStart()
      onAnnounce(null)
      onThreadSelected(typeof createdThread === 'string' && createdThread ? createdThread : null)
      publishFreshThread(true)
      publishMessages([])
      onHistoryLoaded()
      onFollow()
      onFocus()
      onHistoryError('')
      if (fromLauncher) return await submitPrompt(prompt, [], true)
      return true
    } catch (_) {
      if (!destroyed) onHistoryError('A new thread could not be started.', { label: 'Start new thread', run: () => newThread() })
      return false
    } finally {
      switchingThread = false
      if (!destroyed) onThreadSwitch(switchBlocked)
      if (recoveryPending) void recoverChatEvents()
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
    if (!threadId || switchingThread || switchBlocked || holdsThread()) return false
    // The thread on screen counts as open: the shell shows the newest thread's
    // transcript before a selection is recorded, and the sidebar marks it the same way.
    const shownThreadId = readThreadId() ?? (freshThread ? null : readThreadSummaries()[0]?.threadId ?? null)
    const deletingCurrent = threadId === shownThreadId
    // A thread whose run still goes stays until that run settles.
    if ((deletingCurrent && active()) || threadId === background?.threadId) return false
    threadRefreshSequence += 1
    switchingThread = true
    onThreadSwitch(true)
    const summary = readThreadSummaries().find((item) => item.threadId === threadId)
    try {
      const answer = await invoke('chat_delete_thread', { threadId })
      if (destroyed) return false
      if (answer?.folder_kept) onThreadFolderKept(summary ?? { threadId }, answer)
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
      if (recoveryPending) void recoverChatEvents()
    }
  }

  // A thread's selection names the machine its row lives on, so requests
  // and the model list follow the machine the composer shows.
  // The machine a select names. A thread the list no longer names keeps the
  // machine a select last named for it, so a select never falls back to
  // whichever machine is active.
  // A synced copy listed while this computer's runtime did not answer names
  // its holder, never the machine the thread lives on, so its select names
  // no machine and selects no paired machine.
  function selection(threadId) {
    const summary = readThreadSummaries().find((entry) => entry.threadId === threadId)
    if (summary?.replica && !localListed) return { threadId }
    const runtimeId = summary?.runtimeId
      ?? selectedRuntimes.get(threadId)
    if (!runtimeId) return { threadId }
    selectedRuntimes.set(threadId, runtimeId)
    if (selectedRuntimes.size > signaledThreadCap) selectedRuntimes.delete(selectedRuntimes.keys().next().value)
    return { threadId, runtimeId }
  }

  async function sendNewThread(text) {
    const prompt = typeof text === 'string' ? text.trim() : ''
    if (!prompt) return false
    return newThread(prompt)
  }

  async function send() {
    const prompt = readDraft().trim()
    if (!prompt || active() || switchingThread || switchBlocked || blocked()) return
    if (backgroundHere()) {
      onSubmitError(runElsewhere)
      return
    }
    const selected = readFiles()
    const folders = selected.filter(file => file.isDirectory).map(file => file.path)
    return submitPrompt(withFolders(prompt, folders), selected.filter(file => !file.isDirectory).map(({ path }) => ({ path })), false, readComposer())
  }

  async function submitWhenAvailable(payload) {
    const started = Date.now()
    const deadline = started + 30_000
    for (;;) {
      try {
        onSubmission({ invoke: 'pending', error: 'none', source: 'none' })
        const result = await invoke('chat_submit', payload)
        onSubmission({ invoke: 'accepted', error: 'none', source: 'none' })
        return result
      } catch (error) {
        const { source, message } = submissionFailure(error)
        const kind = message === desktopBusy ? 'busy'
          : message === 'Muniment cannot reach its background service.' ? 'unavailable'
            : ['Authorization failed. Sign in again.', 'The runtime refused the request as unauthorized. Enter local mode or sign in, then retry.'].includes(message) ? 'unauthorized' : 'rejected'
        onSubmission({ invoke: 'rejected', error: kind, source })
        // DesktopBusy means the holder sent no request. History can hold that connection during a send.
        // A settled run frees the runtime's run slot just after its last event,
        // so a reply in progress gets a short retry too.
        // Never retry a transport failure because the runtime might already own the run.
        const retryable = message === desktopBusy || (message === replyInProgress && Date.now() < started + replyInProgressRetryMs)
        if (!retryable || Date.now() >= deadline || destroyed) throw error
        await new Promise(resolve => setTimeout(resolve, 100))
        if (destroyed) throw error
      }
    }
  }

  // `composer` is the draft and files the send took from the composer. They
  // leave it in the same update that shows the sent message, and a send the
  // runtime refuses puts them back while the composer stays empty.
  async function submitPrompt(prompt, files, preserveDraft = false, composer = null) {
    onSend()
    onSubmitError('')
    const submissionId = ++submissionSequence
    const userMessage = { role: 'user', sentAt: new Date().toISOString(), text: prompt, attachments: [], submissionId }
    // A reply carries no start time until the runtime's first event names the
    // run's start, which its receipt times from, so its live time never jumps back.
    const pending = { id: 'pending', phase: 'thinking', text: '', receipt: null, prompt, submissionId }
    publishMessages([...messages(), userMessage, { role: 'assistant', run: pending }])
    if (composer) {
      onDraft('')
      onFiles([])
    }
    const restoreComposer = () => {
      if (!composer || submissionId !== submissionSequence || readDraft().trim() || readFiles().length) return
      onDraft(composer.text)
      onFiles(composer.files)
    }
    onActive(pending)
    onAnnounce(pending)
    onFollow()
    holdBuffer()
    try {
      const preparation = prepareSubmit({ prompt, files })
      if (preparation) await preparation
      if (destroyed || submissionId !== submissionSequence) return
      const thinkingLevel = readThinkingLevel()
      // A launcher thread takes no mode from the main composer.
      const permissionMode = preserveDraft ? null : readPermissionMode()
      const model = preserveDraft ? null : readPendingModel()
      const mentions = preserveDraft ? [] : readMentions(prompt)
      const run = await submitWhenAvailable({ prompt, files, ...(thinkingLevel ? { thinkingLevel } : {}), ...(permissionMode ? { permissionMode } : {}), ...(model ? { model } : {}), ...(mentions.length ? { mentions } : {}) })
      if (destroyed || submissionId !== submissionSequence) return
      onSubmitted()
      if (run.mentions?.length) onMentions(run.mentions)
      publishMessages(messages().map((message) => message.submissionId === submissionId ? { ...message, attachments: run.attachments ?? [] } : message))
      const identified = { ...pending, id: run.runId }
      const projected = applyLiveEvents(identified, buffered.get(run.runId) ?? [])
      buffered.delete(run.runId)
      publishMessages(messages().map((message) => message.run?.submissionId === submissionId ? { ...message, run: projected } : message))
      onAnnounce(projected)
      const settled = settledPhases.has(projected.phase)
      onActive(settled ? null : projected)
      if (settled) await refreshThreads()
      return true
    } catch (error) {
      if (destroyed) return
      const { message: reason } = submissionFailure(error)
      if (reason === replyInProgress) {
        // The runtime recorded nothing, so the turn leaves the transcript and
        // the prompt waits in the composer.
        publishMessages(messages().filter((message) => message.submissionId !== submissionId && message.run?.submissionId !== submissionId))
        if (submissionId !== submissionSequence) return false
        if (preserveDraft && !readDraft().trim()) onDraft(prompt)
        restoreComposer()
        onAnnounce(null)
        onActive(null)
        onSubmitError(runElsewhere)
        return false
      }
      if (reason === threadElsewhere && postElsewhere && submissionId === submissionSequence) {
        // The runtime recorded nothing; the message goes to the machine that
        // runs the thread instead.
        publishMessages(messages().filter((message) => message.submissionId !== submissionId && message.run?.submissionId !== submissionId))
        onAnnounce(null)
        onActive(null)
        if (await postElsewhere(prompt)) return true
        restoreComposer()
        return false
      }
      if (deliveryFailureReason || recoveryPending || [...buffered.values()].some((events) => events.some((event) => event.threadId && (!readThreadId() || event.threadId === readThreadId())))) {
        // A lost acknowledgment does not prove that the runtime rejected the run.
        const restoring = { ...pending, id: `recovering-${submissionId}`, phase: 'recovering' }
        publishMessages(messages().map((message) => message.run?.submissionId === submissionId ? { ...message, run: restoring } : message))
        onActive(restoring)
        onAnnounce(restoring)
        recoveryPending = true
        restoreComposer()
        return false
      }
      const { message } = submissionFailure(error)
      const failed = { ...pending, id: `rejected-${messages().length}`, phase: 'failed', failureReason: typeof message === 'string' ? message : 'The message could not be sent.' }
      publishMessages(messages().map((message) => message.run?.submissionId === submissionId ? { ...message, run: failed } : message))
      restoreComposer()
      if (submissionId !== submissionSequence) return
      onAnnounce(failed)
      onActive(null)
      return false
    } finally {
      releaseBuffer()
      if (recoveryPending) void recoverChatEvents()
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
    if (active() || background || blocked() || !run.resumable || run.phase !== 'interrupted') return
    const resuming = { ...run, phase: 'resuming', resumeError: '' }
    onActive(resuming)
    onAnnounce(resuming)
    publishMessages(messages().map((message) => message.run?.id === run.id ? { ...message, run: resuming } : message))
    holdBuffer()
    try {
      await invoke('chat_resume', { runId: run.id })
      if (destroyed) return
      const current = messages().find((message) => message.run?.id === run.id)?.run ?? resuming
      const projected = applyLiveEvents(current, buffered.get(run.id) ?? [])
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
    const threadId = readThreadId()
    try {
      await invoke('chat_queue', { runId: run.id, delivery, message })
      if (destroyed) return
      // The runtime took the message into the running reply, so it shows
      // inside that reply where its text stood, never after it as a turn
      // with no answer. It joins the run's own transcript, not one the user
      // opened since.
      const index = runIndex(run.id)
      const shown = index < 0 ? run : messages()[index].run
      const steer = { text: message, textOffset: (shown.text ?? '').length }
      onSteer(run.id, steer)
      if (readThreadId() !== threadId) return
      if (index >= 0) replaceRun(index, { ...shown, steers: [...(shown.steers ?? []), steer] })
      const running = active()
      if (running?.id === run.id) onActive({ ...running, steers: [...(running.steers ?? []), steer] })
      onFollow()
      if (readDraft().trim() === message) onDraft('')
    } catch (error) {
      if (!destroyed) onQueueError(typeof error === 'string' ? error : String(error))
    }
  }

  function cleanup() {
    destroyed = true
    clearTimeout(recoveryRetry)
    threadRefreshSequence += 1
    unlisten?.()
    unlisten = undefined
    buffered.clear()
    liveText.clear()
  }

  return { start, loadHistory, loadOlderThreads, openThread: (threadId, onFailure) => typeof onFailure === 'function' ? openThread(threadId, true, false, true, onFailure) : openThread(threadId, true), refreshOpenThread, refreshThreads, recoverChatEvents, historyReadFailed: () => historyLoadFailed, newThread: () => newThread(), sendNewThread, renameThread, deleteThread, send, cancel, resume, queue, cleanup }
}
