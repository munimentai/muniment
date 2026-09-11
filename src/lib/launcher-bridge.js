// The main window owns thread selection and submission. The launcher never
// changes the shared backend selection while another window submits a message.
export async function listenForLauncher({ listen, emitTo, ready, send }) {
  let busy = false
  return listen('launcher-submit', async ({ payload }) => {
    if (typeof payload?.id !== 'string' || typeof payload?.text !== 'string') return
    const { id, text } = payload
    let error = ''
    if (busy) error = 'Wait for the current message to send.'
    else {
      busy = true
      try {
        if (!await ready()) error = 'Open the main window to set up a model.'
        else if (!await send(text)) error = 'The message did not send. Check the main window and retry.'
      } catch (_) {
        error = 'The message did not send. Check the main window and retry.'
      } finally {
        busy = false
      }
    }
    await emitTo('launcher', 'launcher-result', { id, error })
  })
}
