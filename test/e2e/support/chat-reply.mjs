// Reads one chat response in the page: the assistant text it renders, streaming
// or completed, and the failure it shows. The caret a streaming reply draws is
// not text, and nothing outside the response counts, so the user's prompt never
// reads as a reply. The function runs in the WebView through browser.execute,
// so it takes only its argument and closes over nothing.
export function replySnapshot(response) {
  if (!response) return { text: '', error: '', streaming: false }
  const text = [...response.querySelectorAll('.assistant-markdown')]
    .filter((block) => !block.closest('.user-turn'))
    .map((block) => {
      const copy = block.cloneNode(true)
      for (const caret of copy.querySelectorAll('.caret')) caret.remove()
      return copy.textContent.trim()
    })
    .filter(Boolean)
    .join('\n')
  const error = [...response.querySelectorAll('.run-error')]
    .map((failure) => failure.textContent.replace(/\s+/g, ' ').trim())
    .filter(Boolean)
    .join(' ')
  return { text, error, streaming: !!response.querySelector('.streaming .assistant-markdown') }
}

// Waits for the response to show reply text or a failure. A failure, an empty
// response at the deadline, or no response throws with the prompt named.
export async function waitForReply(response, prompt, { timeout = 180000 } = {}) {
  let reply = { text: '', error: '', streaming: false }
  await browser.waitUntil(async () => {
    if (!(await response.isExisting())) return false
    reply = await browser.execute(replySnapshot, response)
    return reply.error !== '' || reply.text !== ''
  }, {
    timeout,
    timeoutMsg: `local-mode chat did not render its first reply for prompt: ${prompt}`,
  })
  if (reply.error) throw new Error(`The local-mode chat response failed: ${reply.error}`)
  return reply
}
