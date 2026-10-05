import { App } from '@modelcontextprotocol/ext-apps'

const app = new App({ name: 'Workshop quote', version: '1.0.0' }, {})
const seats = document.querySelector('input')
const output = document.querySelector('output')
const status = document.querySelector('[role=status]')
const button = document.querySelector('button')
function show(result) {
  if (result.isError) {
    status.textContent = result.content.find(block => block.type === 'text')?.text || 'The quote failed. Try again.'
    return
  }
  seats.value = result.structuredContent.seats
  output.textContent = `$${result.structuredContent.total}`
  status.textContent = `${result.structuredContent.seats} seats at $25 each.`
}
app.ontoolresult = show
button.onclick = async () => {
  button.disabled = true
  try { show(await app.callServerTool({ name: 'quote', arguments: { seats: seats.valueAsNumber } })) }
  catch { status.textContent = 'The quote failed. Check the connection and retry.' }
  finally { button.disabled = false }
}
void app.connect().catch(() => { status.textContent = 'The app could not connect. Close it and retry.' })
