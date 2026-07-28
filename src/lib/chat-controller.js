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
  onHistoryLoaded = () => {},
  onFollow = () => {},
  onSend = () => {},
}) {
  const buffered = new Map()
  let submissionSequence = 0
  let unlisten
  let destroyed = false

  const messages = () => readMessages()
  const active = () => readActive()
  const publishMessages = (next) => {
    if (!destroyed) onMessages(next)
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
    if (active()?.id === payload.runId) onActive(projected && !settledPhases.has(projected.phase) ? projected : null)
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
      const { summaries } = await invoke('chat_thread_summaries', { limit: 1 })
      if (destroyed) return
      if (!summaries.length) {
        publishMessages([])
        onHistoryLoaded()
        onFollow()
        return
      }
      const history = []
      let cursor
      for (let page = 0; page < historyPageCap; page += 1) {
        const payload = { threadId: summaries[0].threadId, limit: historyPageLimit }
        if (cursor !== undefined) payload.cursor = cursor
        const result = await invoke('chat_thread_open', payload)
        if (destroyed) return
        history.push(...result.entries)
        if (result.nextCursor == null) break
        cursor = result.nextCursor
      }
      if (destroyed) return
      publishMessages(historyMessages(history))
      onHistoryLoaded()
      onFollow()
    } catch (_) {
      if (!destroyed) onHistoryError('Conversation history could not be restored. Try again.')
    }
  }

  async function send() {
    const prompt = readDraft().trim()
    if (!prompt || active() || blocked()) return
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
      onActive(settledPhases.has(projected.phase) ? null : projected)
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
      onActive(settledPhases.has(projected.phase) ? null : projected)
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
    unlisten?.()
    unlisten = undefined
    buffered.clear()
  }

  return { start, loadHistory, send, cancel, resume, queue, cleanup }
}
