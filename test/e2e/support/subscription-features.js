// The installed webview exercises public commands with disposable data.
window.__munimentSubscriptionFeatures = async ({ plan, invoke, wait, setValue, turns }) => {
  const features = {}
  const check = value => { if (!value) throw new Error('The installed feature check failed.') }
  const rejects = async action => {
    let rejected = false
    try { await action() } catch { rejected = true }
    check(rejected)
  }
  const run = async (name, action) => {
    features[name] = []
    try { features[name] = await action() } catch { /* Keep provider errors out of evidence. */ }
  }
  if (plan.phase === 'restart') {
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
  await run('signed-update', async () => {
    await invoke('subscription_probe_update')
    return ['signature-verified', 'tampered-package-rejected', 'signed-version-verified']
  })
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
  await run('artifacts', async () => {
    const folders = await invoke('workspace_folders', { threadId: turns[0].thread })
    const root = folders.at(-1).path
    const paths = await invoke('workspace_file_action', { root, destination: root, action: 'new-file', paths: [], name: 'acceptance.html' })
    const file = await invoke('workspace_read_text', { path: paths[0] })
    const html = `<!doctype html><title>Acceptance artifact</title><p>${plan.nonce}</p>`
    await invoke('workspace_save_text', { path: paths[0], content: html, revision: file.revision })
    const artifact = await invoke('artifact_from_file', { threadId: turns[0].thread, path: paths[0] })
    check((await invoke('artifact_read', { id: artifact.id })).html === html)
    await invoke('artifact_edit', { id: artifact.id, name: 'Renamed acceptance artifact' })
    check((await invoke('artifact_list')).some(item => item.id === artifact.id && item.name === 'Renamed acceptance artifact'))
    return ['html-published', 'artifact-read', 'artifact-renamed']
  })
  await run('browser', async () => {
    await invoke('browser_view', { label: 'browser', bounds: { x: 0, y: 0, width: 640, height: 480 } })
    try {
      await wait(async () => {
        const snapshot = JSON.parse(await invoke('browser_command', { request: { view: 'browser', action: 'snapshot' } }))
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
  const toolTurn = async (prompt, tool, expected) => {
    const thread = turns[0].thread
    const before = await invoke('chat_thread_open', { threadId: thread, limit: 20, cursor: null })
    const composer = document.querySelector('textarea[placeholder="Ask anything"]')
    setValue(composer, prompt)
    const send = await wait(() => [...document.querySelectorAll('button')].find(button =>
      (button.getAttribute('aria-label') === 'Send' || button.textContent.trim() === 'Send') && !button.disabled))
    send.click()
    const entry = await wait(async () => {
      const page = await invoke('chat_thread_open', { threadId: thread, limit: 20, cursor: null })
      const entry = page.entries.find(entry => !before.entries.some(old => old.runId === entry.runId))
      if (entry && ['failed', 'cancelled', 'interrupted', 'pending-permission'].includes(entry.phase)) throw new Error('The tool reply failed.')
      return entry?.phase === 'complete' && entry
    })
    check(entry.text.trim() === expected)
    check(entry.receipt?.tools?.some(item => tool.test(item.name) && item.calls > 0 && item.failed === 0))
  }
  await run('tools', async () => {
    await toolTurn(`Use the read tool to read ${JSON.stringify(plan.fixtureFile)}. Reply with only its first line.`, /^(read|read_file)$/, plan.fileNonce)
    return ['file-tool-completed']
  })
  await run('mcp', async () => {
    const call = (action, data) => invoke('extend_command', { action, data })
    const id = 'release-acceptance'
    await call('server', { id, name: 'Release acceptance', definition: { command: plan.mcpCommand, args: [plan.mcpScript, plan.mcpNonce, plan.mcpReceipt] } })
    try {
      const connection = await call('test', { id })
      check(connection.status === 'connected')
      check(connection.tools.some(tool => tool.name === 'acceptance_token'))
      const extensions = await wait(() => document.querySelector('button[aria-label="Extensions"]'))
      extensions.click()
      const branch = await wait(() => document.querySelector('button[data-branch="mcp"]'))
      branch.click()
      const toggle = await wait(() => document.querySelector('[role="switch"][aria-label="Use Release acceptance for this turn"]'))
      if (toggle.getAttribute('aria-checked') !== 'true') toggle.click()
      await wait(() => toggle.getAttribute('aria-checked') === 'true')
      extensions.click()
      await toolTurn('Call the acceptance_token MCP tool. Reply with only the token from its result.', /acceptance_token|^mcp/, plan.mcpNonce)
      return ['server-connected', 'tool-discovered', 'tool-completed']
    } finally { await call('remove', { id }) }
  })
  return features
}
