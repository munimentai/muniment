import { describe, expect, it, vi } from 'vitest'
import fs from 'node:fs'

const spec = fs.readFileSync('test/e2e/specs/real-sign-in.spec.js', 'utf8')
const block = spec.slice(spec.indexOf('    let receipt\n'), spec.indexOf('    const assistantResponse ='))
const run = new Function('browser', 'response', 'prompt', `return (async () => { ${block} })()`)

function fixture({ refusal, hasReceipt }) {
  const receipt = { isDisplayed: vi.fn(async () => hasReceipt) }
  const failure = { isDisplayed: async () => refusal !== undefined, getText: async () => refusal }
  const response = { $: async (selector) => selector === '.run-error' ? failure : receipt }
  const browser = { waitUntil: vi.fn(async (predicate, options) => {
    if (!await predicate()) throw new Error(options.timeoutMsg)
  }) }
  return { receipt, response, browser }
}

describe('The signed-in spec reads the reply outcome.', () => {
  it.each([false, true])('reports a refusal on the first poll with receipt visibility %s', async (hasReceipt) => {
    const reason = 'chat_not_entitled: No chat model is currently available for this account.'
    const { receipt, response, browser } = fixture({ refusal: reason, hasReceipt })
    expect(block).toContain('let refusal')
    await expect(run(browser, response, 'test prompt')).rejects.toThrow(`The chat response failed: ${reason}`)
    expect(browser.waitUntil).toHaveBeenCalledTimes(1)
    expect(receipt.isDisplayed).not.toHaveBeenCalled()
  })

  it('accepts a receipt when the reply has no refusal', async () => {
    const { receipt, response, browser } = fixture({ hasReceipt: true })
    await expect(run(browser, response, 'test prompt')).resolves.toBeUndefined()
    expect(receipt.isDisplayed).toHaveBeenCalledTimes(1)
  })

  it('does not treat an empty reply as a receipt', async () => {
    const { response, browser } = fixture({ hasReceipt: false })
    await expect(run(browser, response, 'test prompt')).rejects.toThrow('neither a receipt nor a refusal')
  })
})
