// The installed webview exercises public commands with disposable data.
window.__munimentSubscriptionFeatures = async ({ plan, invoke, wait, setValue, turns }) => {
  const features = {}
  const failures = new WeakMap()
  const check = (value, reason = 'check-failed') => {
    if (value) return
    const error = new Error('The installed feature check failed.')
    failures.set(error, reason)
    throw error
  }
  const rejects = async action => {
    let rejected = false
    try { await action() } catch { rejected = true }
    check(rejected)
  }
  const run = async (name, action) => {
    features[name] = []
    let failure
    const step = async (stage, action) => {
      try { return await action() } catch (error) {
        // Keep the first failure when cleanup also fails. Never copy provider text.
        failure ??= ['failed', stage, failures.get(error) ?? (['timeout', 'command-timeout'].includes(error?.errorClass) ? 'timeout' : 'check-failed')]
        throw error
      }
    }
    try { features[name] = await step('check', () => action(step)) } catch { features[name] = failure }
  }
  if (plan.phase === 'update') {
    const codes = ['update-profile', 'update-plan', 'update-phase', 'update-state', 'update-address',
      'update-builder', 'update-check', 'update-download', 'update-unavailable', 'update-not-prepared',
      'update-package-digest', 'update-tamper-rejection', 'update-version-rejection', 'update-active-work-refusal',
      'update-checkpoint-encode', 'update-checkpoint-write', 'update-busy', 'update-install-task', 'update-install', 'update-restart']
    try {
      await invoke('subscription_probe_update')
    } catch (error) {
      // Only fixed Rust codes enter the evidence. Never copy an unknown rejection.
      const errorClass = codes.includes(error) ? error : error?.errorClass === 'command-timeout' ? 'command-timeout' : 'command-failed'
      throw Object.assign(new Error('The installed update failed.'), { errorClass })
    }
    throw Object.assign(new Error('The updated app did not restart.'), { errorClass: 'update-restart' })
  }
  if (['restart', 'update-restart'].includes(plan.phase)) {
    await run('restart-persistence', async () => {
      const thread = await invoke('chat_current_thread')
      check(thread === plan.turns[0].thread)
      const page = await invoke('chat_thread_open', { threadId: thread, limit: 20, cursor: null })
      check(plan.turns.every(turn => page.entries.some(entry => entry.runId === turn.run && entry.text.trim() === plan.nonce)))
      const inventory = await invoke('local_mode_provider_inventory')
      const model = plan.models.at(-1)
      check(inventory.default_provider === 'muniment-router' && inventory.default_model === `${model.family}/${model.id}`)
      check((await invoke('workspace_read_text', { path: plan.fixtureFile })).content === `${plan.fileNonce}\nEdited\n`)
      const settings = await invoke('model_router_settings')
      check(settings.accounts[0]?.label === 'Acceptance account')
      return ['thread-restored', 'model-restored', 'file-restored', 'settings-restored']
    })
    if (plan.phase === 'update-restart') {
      features['signed-update'] = features['restart-persistence'].length === 4
        ? ['signature-verified', 'tampered-package-rejected', 'signed-version-verified', 'busy-install-rejected', 'app-relaunched', 'profile-restored'] : ['failed', 'restore', 'check-failed']
    }
    return features
  }
  if (plan.phase === 'chat') {
    await run('account-balancing', async () => {
      const settings = await wait(async () => {
        const settings = await invoke('model_router_settings')
        return settings.accounts.every(account => account.active === 0) && settings
      })
      const used = settings.accounts.filter(account => account.source === 'account' && account.requests > 0)
      check(used.some(account => used.some(other => other.id !== account.id && other.family === account.family)))
      for (const account of used) {
        const pool = settings.accounts.filter(other => other.family === account.family && other.enabled)
        check(pool.every(other => other.weight === 1))
        check(Math.max(...pool.map(other => other.requests)) - Math.min(...pool.map(other => other.requests)) <= 1)
      }
      check(settings.accounts.every(account => account.active === 0 && account.errors === 0))
      return ['multiple-accounts-served', 'equal-weight-shares', 'no-active-reservations']
    })
    return features
  }
  await run('settings', async () => {
    const before = await invoke('model_router_settings')
    const id = before.accounts[0].id
    await invoke('model_router_update_account', { id, label: 'Acceptance account', weight: 2 })
    await rejects(() => invoke('model_router_update_account', { id, label: '' }))
    const saved = await invoke('model_router_settings')
    check(saved.accounts.find(account => account.id === id)?.label === 'Acceptance account')
    check(saved.accounts.find(account => account.id === id)?.weight === 2)
    return ['account-edit-saved', 'invalid-edit-rejected']
  })
  await run('routing', async () => {
    const before = await invoke('model_router_settings')
    const model = plan.models[0]
    const fallback = `${model.family}/${model.id}`
    try {
      await invoke('model_router_save_routes', { routes: before.routes, fallback, minConfidence: 0.6 })
      const decision = await invoke('model_router_test_route', { sample: 'Return the acceptance token.' })
      check(decision.model === fallback && decision.fallback_reason && decision.eligible_models.includes(fallback))
      await rejects(() => invoke('model_router_test_route', { sample: '' }))
      await rejects(() => invoke('model_router_save_routes', { routes: before.routes, fallback: 'openai/nonexistent-acceptance-model' }))
      for (const account of before.accounts.filter(account => account.family === model.family)) {
        const models = account.models.filter(id => id !== model.id)
        await invoke('model_router_update_account', { id: account.id, models, enabled: account.enabled && models.length > 0 })
      }
      const next = plan.models[1]
      await invoke('model_router_save_routes', { routes: before.routes, fallback: `${next.family}/${next.id}` })
      const after = await invoke('model_router_test_route', { sample: 'Return the acceptance token.' })
      check(!after.eligible_models.includes(fallback) && after.model === `${next.family}/${next.id}`)
      return ['fallback-selected', 'empty-sample-rejected', 'unavailable-model-excluded']
    } finally {
      for (const account of before.accounts.filter(account => account.family === model.family)) {
        await invoke('model_router_update_account', { id: account.id, models: account.models, enabled: account.enabled })
      }
      await invoke('model_router_save_routes', { routes: before.routes, fallback: before.fallback, minConfidence: before.min_confidence })
    }
  })
  await run('files', async () => {
    const original = await invoke('workspace_read_text', { path: plan.fixtureFile })
    check(original.content === plan.fileNonce)
    const content = `${plan.fileNonce}\nEdited\n`
    await invoke('workspace_save_text', { path: plan.fixtureFile, content, revision: original.revision })
    await rejects(() => invoke('workspace_save_text', { path: plan.fixtureFile, content: 'stale', revision: original.revision }))
    check((await invoke('workspace_read_text', { path: plan.fixtureFile })).content === content)
    return ['file-read', 'file-edit-saved', 'stale-edit-rejected']
  })
  await run('projects', async () => {
    const id = await invoke('project_create', { name: 'Acceptance project' })
    check((await invoke('project_list')).projects[id] === 'Acceptance project')
    await invoke('project_rename', { projectId: id, name: 'Renamed acceptance project' })
    check((await invoke('project_list')).projects[id] === 'Renamed acceptance project')
    return ['project-created', 'project-renamed']
  })
  await run('memory', async () => {
    const before = await invoke('memory_profile_read')
    await invoke('memory_profile_save', { content: '# Profile\n\nAcceptance profile.\n' })
    check((await invoke('memory_profile_read')).includes('Acceptance profile.'))
    await invoke('memory_profile_save', { content: before })
    check(await invoke('memory_profile_read') === before)
    return ['profile-saved', 'profile-restored']
  })
  await run('agents', async () => {
    const agent = await invoke('agent_save', { agent: { name: 'Acceptance agent', instructions: 'Return the test token.' } })
    check((await invoke('agent_list')).agents.some(item => item.id === agent.id && item.instructions === agent.instructions))
    const renamed = await invoke('agent_save', { agent: { ...agent, name: 'Renamed acceptance agent' } })
    check(renamed.id === agent.id && renamed.name === 'Renamed acceptance agent')
    await invoke('agent_delete', { id: agent.id })
    check(!(await invoke('agent_list')).agents.some(item => item.id === agent.id))
    return ['agent-saved', 'agent-renamed', 'agent-deleted']
  })
  await run('artifacts', async step => {
    const root = await step('folders', async () => {
      const folders = await invoke('workspace_folders', { threadId: turns[0].thread })
      check(folders.at(-1)?.path)
      return folders.at(-1).path
    })
    const filePath = await step('new-file', async () => {
      const paths = await invoke('workspace_file_action', { root, destination: root, action: 'new-file', paths: [], name: 'acceptance.html' })
      check(paths[0])
      return paths[0]
    })
    const html = `<!doctype html><title>Acceptance artifact</title><p>${plan.nonce}</p>`
    await step('save', async () => {
      const file = await invoke('workspace_read_text', { path: filePath })
      await invoke('workspace_save_text', { path: filePath, content: html, revision: file.revision })
    })
    const artifact = await step('publish', () => invoke('artifact_from_file', { threadId: turns[0].thread, path: filePath }))
    await step('read', async () => check((await invoke('artifact_read', { id: artifact.id })).html === html))
    await step('rename', async () => {
      await invoke('artifact_edit', { id: artifact.id, name: 'Renamed acceptance artifact' })
      check((await invoke('artifact_list')).some(item => item.id === artifact.id && item.name === 'Renamed acceptance artifact'))
    })
    return ['html-published', 'artifact-read', 'artifact-renamed']
  })
  await run('browser', async () => {
    await invoke('browser_view', { label: 'browser', bounds: { x: 0, y: 0, width: 640, height: 480 } })
    try {
      await wait(async () => {
        let response
        try {
          response = await invoke('browser_command', { request: { view: 'browser', action: 'snapshot' } })
        } catch (error) {
          if ((error?.message ?? error) === 'The page is still loading.') return false
          throw error
        }
        const snapshot = JSON.parse(response)
        return snapshot.title && snapshot.text.length > 20 && new URL(snapshot.url).hostname === '127.0.0.1'
      })
      await rejects(() => invoke('browser_command', { request: { view: 'browser', action: 'navigate', value: 'file:///etc/passwd' } }))
    } finally {
      await invoke('browser_command', { request: { view: 'browser', action: 'close' } })
      await invoke('browser_view', { label: '' })
    }
    return ['local-page-rendered', 'unsafe-navigation-rejected', 'view-closed']
  })
  await run('terminal', async () => {
    const id = await invoke('terminal_start', { path: plan.fixtureDirectory, cols: 80, rows: 24 })
    try {
      // Ignore the echoed command. Require the shell's separate output line.
      await invoke('terminal_write', { id, data: `echo ${plan.nonce}\r` })
      let text = ''
      await wait(async () => {
        const output = await invoke('terminal_read', { id })
        text += String.fromCharCode(...output.bytes)
        return text.replace(/\r/g, '').split('\n').some(line => line === plan.nonce)
      })
    } finally { await invoke('terminal_close', { id }) }
    await rejects(() => invoke('terminal_read', { id }))
    return ['shell-output', 'shell-closed']
  })
  const toolTurn = async (prompt, tool, expected, approvalTitle) => {
    const thread = turns[0].thread
    const before = await invoke('chat_thread_open', { threadId: thread, limit: 20, cursor: null })
    const composer = document.querySelector('textarea[placeholder="Ask anything"]')
    setValue(composer, prompt)
    const send = await wait(() => [...document.querySelectorAll('button')].find(button =>
      (button.getAttribute('aria-label') === 'Send' || button.textContent.trim() === 'Send') && !button.disabled))
    send.click()
    let runId, approvedGate, phase
    let entry
    try {
      entry = await wait(async () => {
        const page = await invoke('chat_thread_open', { threadId: thread, limit: 20, cursor: null })
        const entry = page.entries.find(entry => runId ? entry.runId === runId : !before.entries.some(old => old.runId === entry.runId))
        if (!entry) return false
        runId = entry.runId
        phase = entry.phase
        if (phase === 'interrupted') {
          // Export only fixed journal reasons. Keep provider text out of the evidence.
          const reason = ['unknown-effect-outcome', 'interrupted', 'unspecified'].includes(entry.failureReason)
            ? entry.failureReason : entry.failureReason ? 'recorded' : 'missing-reason'
          check(false, `reply-interrupted-${reason}-${approvedGate ? 'approved' : 'unapproved'}`)
        }
        for (const terminal of ['failed', 'cancelled']) {
          check(phase !== terminal, `reply-phase-${terminal}`)
        }
        if (phase === 'pending-permission') {
          const gate = entry.pendingPermission
          // Approve only this fixture call, once. Keep every other gate closed.
          check(approvalTitle && gate?.kind === 'select' && gate.title === approvalTitle &&
            typeof gate.gateId === 'string' && gate.gateId.length > 0 &&
            Array.isArray(gate.options) && gate.options.includes('Allow once') &&
            (!approvedGate || approvedGate === gate.gateId), 'reply-phase-pending-permission')
          if (!approvedGate) {
            approvedGate = gate.gateId
            try {
              await invoke('chat_answer_permission', { runId, gateId: gate.gateId, answer: { type: 'select', value: 'Allow once' } })
            } catch { check(false, 'reply-phase-pending-permission') }
          }
        }
        return phase === 'complete' && entry
      })
    } catch (error) {
      if (phase === 'pending-permission' && error?.errorClass === 'timeout') check(false, 'reply-phase-pending-permission')
      throw error
    }
    check(typeof entry.text === 'string' && entry.text.trim() === expected, 'reply-text')
    check(Array.isArray(entry.receipt?.tools) && entry.receipt.tools.some(item =>
      typeof item?.name === 'string' && tool.test(item.name) && Number.isSafeInteger(item.calls) && item.calls > 0 && item.failed === 0), 'receipt-tool')
  }
  await run('tools', async () => {
    await toolTurn(`Use the read tool to read ${JSON.stringify(plan.fixtureFile)}. Reply with only its first line.`, /^(read|read_file)$/, plan.fileNonce)
    return ['file-tool-completed']
  })
  await run('mcp', async step => {
    const call = (action, data) => invoke('extend_command', { action, data })
    const id = 'release-acceptance'
    await step('server-add', () => call('server', { id, name: 'Release acceptance', definition: { command: plan.mcpCommand, args: [plan.mcpScript, plan.mcpNonce, plan.mcpReceipt] } }))
    try {
      await step('connection-test', async () => {
        const connection = await call('test', { id })
        check(connection.status === 'connected')
        check(connection.tools.some(tool => tool.name === 'acceptance_token'))
      })
      await step('toggle', async () => {
        const extensions = await wait(() => document.querySelector('button[aria-label="Extensions"]'))
        extensions.click()
        const branch = await wait(() => document.querySelector('button[data-branch="mcp"]'))
        branch.click()
        const toggle = await wait(() => document.querySelector('[role="switch"][aria-label="Use Release acceptance for this turn"]'))
        if (toggle.getAttribute('aria-checked') !== 'true') toggle.click()
        await wait(() => toggle.getAttribute('aria-checked') === 'true')
        extensions.click()
      })
      // A new lazy server has no cached tool names. Scope the call to that server.
      const approvalTitle = `MCP: extend-${id} wants to run acceptance_token\n\nArguments:\n{}\n\nAllow server for this session permits all tools and arguments on this server until reload or session/branch change. Other security and UI consent checks still apply.`
      await step('tool-turn', () => toolTurn(`Call the MCP gateway tool mcp with {"server":"extend-${id}","tool":"acceptance_token","args":{}}. Reply with only the token from its result.`, /acceptance_token|^mcp/, plan.mcpNonce, approvalTitle))
      return ['server-connected', 'tool-discovered', 'tool-completed']
    } finally { await step('server-remove', () => call('remove', { id })) }
  })
  return features
}
