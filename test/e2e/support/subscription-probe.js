// The release app runs this fixed diagnostic in its own main webview.
// It never accepts JavaScript from the runner or exports conversation text.
;(async () => {
  if (window.__munimentSubscriptionProbeStarted) return
  window.__munimentSubscriptionProbeStarted = true
  const plan = window.__MUNIMENT_SUBSCRIPTION_PLAN__
  const invoke = async (command, payload) => {
    let timer
    const timeout = command === 'subscription_probe_update' ? 300_000 : 180_000
    try {
      return await Promise.race([
        window.__TAURI__.core.invoke(command, payload),
        new Promise((_, reject) => { timer = setTimeout(() => reject(failure('command-timeout')), timeout) }),
      ])
    } finally { clearTimeout(timer) }
  }
  const failure = errorClass => Object.assign(new Error('The installed subscription probe failed.'), { errorClass })
  let stage = 'composer', turn = null
  const progress = async (next, errorClass = 'none') => {
    stage = next
    await invoke('subscription_probe_progress', { stage, turn, errorClass })
  }
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
    throw failure('timeout')
  }
  const setValue = (element, value) => {
    element.value = value
    element.dispatchEvent(new Event('input', { bubbles: true }))
  }
  try {
    await progress('composer')
    await wait(() => visible(document.querySelector('textarea#composer-message')))
    await progress('runtime')
    await wait(async () => {
      const status = await invoke('attach_listener_status')
      return status.supervisor_running === true && status.connected === true
    })
    for (let index = 0; !restored && index < plan.models.length; index++) {
      const model = plan.models[index]
      turn = index
      await progress('selection')
      if (index > 0) {
        const chip = await wait(() => document.querySelector('.model-chip'))
        chip.click()
        const row = await wait(() => [...document.querySelectorAll('.picker-row')].find(row =>
          row.dataset.provider === 'muniment-router' && row.dataset.model === `${model.family}/${model.id}`))
        row.click()
        await wait(() => !document.querySelector('[data-panel="model-picker"]'))
      }
      await progress('inventory')
      await wait(async () => {
        const inventory = await invoke('local_mode_provider_inventory')
        return inventory.default_provider === 'muniment-router' && inventory.default_model === `${model.family}/${model.id}`
      })
      await progress('send')
      const composer = document.querySelector('textarea#composer-message')
      const prompt = index === 0
        ? `Remember this test token: ${plan.nonce}. Reply with only that token. Do not use tools.`
        : 'Reply with only the test token from the first message. Do not use tools.'
      setValue(composer, prompt)
      const send = await wait(() => [...document.querySelectorAll('button')].find(button =>
        button.getAttribute('aria-label') === 'Send' && visible(button) && !button.disabled && button.getAttribute('aria-disabled') !== 'true'))
      send.click()
      await progress('reply')
      let thread
      const entry = await wait(async () => {
        thread = await invoke('chat_current_thread')
        if (!thread) return false
        const page = await invoke('chat_thread_open', { threadId: thread, limit: 10, cursor: null })
        if (page.entries.length !== index + 1) return false
        const entry = page.entries.find(entry => !turns.some(turn => turn.run === entry.runId))
        if (!entry) return false
        if (['failed', 'cancelled', 'interrupted', 'pending-permission'].includes(entry.phase)) {
          throw failure('reply-failed')
        }
        return entry.phase === 'complete' && entry
      })
      if (entry.text.trim() !== plan.nonce || (turns.length && thread !== turns[0].thread)) {
        throw failure('context-mismatch')
      }
      await progress('render')
      await wait(() => {
        const responses = [...document.querySelectorAll('.response')]
        return responses.length === index + 1 && responses.every(response =>
          visible(response) && response.querySelector('.assistant-markdown')?.textContent.trim() === plan.nonce && response.querySelector('.provenance'))
      })
      turns.push({ index, thread, run: entry.runId, rendered: true, context: true })
      await progress('complete')
    }
    if (restored) {
      await progress('restore')
      await wait(async () => {
        if (await invoke('chat_current_thread') !== turns[0].thread) return false
        const responses = [...document.querySelectorAll('.response')].filter(response =>
          response.querySelector('.assistant-markdown')?.textContent.trim() === plan.nonce)
        return responses.length === 4 && responses.every(visible)
      })
    }
    if (plan.acceptance) {
      await progress('features')
      features = await window.__munimentSubscriptionFeatures({ plan, invoke, wait, setValue, turns })
      if (plan.phase === 'chat') features['local-startup'] = ['composer-visible', 'runtime-connected']
    }
    // Show only verified synthetic replies. Hide account details and tool output.
    const style = document.createElement('style')
    style.textContent = 'body * { visibility: hidden !important } [data-subscription-evidence], [data-subscription-evidence] * { visibility: visible !important }'
    for (const response of [...document.querySelectorAll('.response')]) {
      if (response.querySelector('.assistant-markdown')?.textContent.trim() === plan.nonce) response.setAttribute?.('data-subscription-evidence', '')
    }
    document.head.append(style)
    document.querySelector('[data-subscription-evidence]')?.scrollIntoView?.({ block: 'start' })
    await progress('result')
    await invoke('subscription_probe_observed', { turns, features, passed: true })
  } catch (error) {
    try { await progress(stage, error?.errorClass || 'command-failed') } catch { /* Save the result even if progress fails. */ }
    await invoke('subscription_probe_observed', { turns, features, passed: false })
  }
})()
