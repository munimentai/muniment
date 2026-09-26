// The release app runs this fixed diagnostic in its own main webview.
// It never accepts JavaScript from the runner or exports conversation text.
;(async () => {
  if (window.__munimentSubscriptionProbeStarted) return
  window.__munimentSubscriptionProbeStarted = true
  const plan = window.__MUNIMENT_SUBSCRIPTION_PLAN__
  const invoke = window.__TAURI__.core.invoke
  const restored = ['features', 'restart', 'update', 'update-restart'].includes(plan.phase)
  const turns = restored ? plan.turns : []
  let features = {}
  const visible = element => element && element.getClientRects().length > 0
  const wait = async predicate => {
    const deadline = Date.now() + 180_000
    while (Date.now() < deadline) {
      const value = await predicate()
      if (value) return value
      await new Promise(resolve => setTimeout(resolve, 250))
    }
    throw new Error('The installed subscription probe timed out.')
  }
  const setValue = (element, value) => {
    element.value = value
    element.dispatchEvent(new Event('input', { bubbles: true }))
  }
  try {
    await wait(() => visible(document.querySelector('textarea[placeholder="Ask anything"]')))
    await wait(async () => {
      const status = await invoke('attach_listener_status')
      return status.supervisor_running === true && status.connected === true
    })
    for (let index = 0; !restored && index < plan.models.length; index++) {
      const model = plan.models[index]
      if (index > 0) {
        const chip = await wait(() => document.querySelector('.model-chip'))
        chip.click()
        const row = await wait(() => [...document.querySelectorAll('.picker-row')].find(row =>
          row.dataset.provider === 'muniment-router' && row.dataset.model === `${model.family}/${model.id}`))
        row.click()
        await wait(() => !document.querySelector('[data-panel="model-picker"]'))
      }
      await wait(async () => {
        const inventory = await invoke('local_mode_provider_inventory')
        return inventory.default_provider === 'muniment-router' && inventory.default_model === `${model.family}/${model.id}`
      })
      const composer = document.querySelector('textarea[placeholder="Ask anything"]')
      const prompt = index === 0
        ? `Remember this test token: ${plan.nonce}. Reply with only that token. Do not use tools.`
        : 'Reply with only the test token from the first message. Do not use tools.'
      setValue(composer, prompt)
      const send = await wait(() => [...document.querySelectorAll('button')].find(button =>
        (button.getAttribute('aria-label') === 'Send' || button.textContent.trim() === 'Send') && visible(button) && !button.disabled))
      send.click()
      let thread
      const entry = await wait(async () => {
        thread = await invoke('chat_current_thread')
        if (!thread) return false
        const page = await invoke('chat_thread_open', { threadId: thread, limit: 10, cursor: null })
        if (page.entries.length !== index + 1) return false
        const entry = page.entries.find(entry => !turns.some(turn => turn.run === entry.runId))
        if (!entry) return false
        if (['failed', 'cancelled', 'interrupted', 'pending-permission'].includes(entry.phase)) {
          throw new Error('The installed subscription reply failed.')
        }
        return entry.phase === 'complete' && entry
      })
      if (entry.text.trim() !== plan.nonce || (turns.length && thread !== turns[0].thread)) {
        throw new Error('The model switch lost the test context.')
      }
      await wait(() => {
        const responses = [...document.querySelectorAll('.response')]
        return responses.length === index + 1 && responses.every(response =>
          visible(response) && response.querySelector('.response-prose')?.textContent.trim() === plan.nonce && response.querySelector('.provenance'))
      })
      turns.push({ index, thread, run: entry.runId, rendered: true, context: true })
    }
    if (restored) {
      await wait(async () => {
        if (await invoke('chat_current_thread') !== turns[0].thread) return false
        const responses = [...document.querySelectorAll('.response')].filter(response =>
          response.querySelector('.response-prose')?.textContent.trim() === plan.nonce)
        return responses.length === 4 && responses.every(visible)
      })
    }
    if (plan.acceptance) {
      features = await window.__munimentSubscriptionFeatures({ plan, invoke, wait, setValue, turns })
      if (plan.phase === 'chat') features['local-startup'] = ['composer-visible', 'runtime-connected']
    }
    // Show only verified synthetic replies. Hide account details and tool output.
    const style = document.createElement('style')
    style.textContent = 'body * { visibility: hidden !important } [data-subscription-evidence], [data-subscription-evidence] * { visibility: visible !important }'
    for (const response of [...document.querySelectorAll('.response')]) {
      if (response.querySelector('.response-prose')?.textContent.trim() === plan.nonce) response.setAttribute?.('data-subscription-evidence', '')
    }
    document.head.append(style)
    document.querySelector('[data-subscription-evidence]')?.scrollIntoView?.({ block: 'start' })
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))
    await invoke('subscription_probe_observed', { turns, features, passed: true })
  } catch {
    await invoke('subscription_probe_observed', { turns, features, passed: false })
  }
})()
