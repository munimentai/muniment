import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { agentToolChoice, emptyCatalog, loadToolCatalog, missingSkills, projectTools, serverTools } from './agent-tools.js'

const fixture = (name) => JSON.parse(fs.readFileSync(path.join(process.cwd(), 'core/protocol-fixtures/muniment.attach/1', name), 'utf8')).body
const answer = fixture('response-agent-tool-catalog.json')
const tauri = (reply) => ({ invoke: vi.fn(async () => { if (reply instanceof Error) throw reply; return reply }) })

describe('agent tool catalog', () => {
  it('asks the agent\'s own machine and maps core\'s answer', async () => {
    const client = tauri(answer)
    const catalog = await loadToolCatalog(client, 'box')
    expect(client.invoke).toHaveBeenCalledWith('agent_request', { runtime: 'box', operation: 'agent.tool_catalog', body: {} })
    expect(catalog.builtin.map((tool) => tool.id)).toEqual(['read', 'write', 'bash', 'web_search'])
    expect(catalog.builtin[0]).toEqual({ id: 'read', label: 'Read files', readOnly: true })
    expect(catalog.mcp).toEqual([
      { server: 'Leads', tools: ['search_leads', 'get_lead'], checkedAt: '2026-01-01T08:55:00Z' },
      { server: 'Calendar', tools: [], checkedAt: null },
    ])
    expect(catalog.skills[0]).toMatchObject({ name: 'review', description: 'Review a draft against the house style.' })
    expect(catalog.loaded).toBe(true)
  })

  it('keeps the machine\'s refusal as the reason', async () => {
    const catalog = await loadToolCatalog(tauri(new Error('That machine is offline. Try again when it answers.')), 'box')
    expect(catalog).toMatchObject({ builtin: [], loaded: true, error: 'That machine is offline. Try again when it answers.' })
  })

  it('records an agent choice only where it differs from its project', () => {
    const policy = { disabledTools: ['bash'], disabledExtensions: [] }
    const bash = { id: 'bash', kind: 'builtin' }
    const crm = { id: 'mcp-crm', kind: 'mcp' }
    const allowed = agentToolChoice({ tools: { disabled: ['write'] } }, policy, bash, true)
    expect(allowed).toEqual({ disabled: ['write'], disabled_extensions: [], allowed: ['bash'] })
    expect(agentToolChoice({ tools: allowed }, policy, bash, false)).toEqual({ disabled: ['write'], disabled_extensions: [], allowed: [] })
    expect(agentToolChoice({}, policy, crm, false)).toEqual({ disabled: [], disabled_extensions: ['mcp-crm'], allowed: [] })
    expect(agentToolChoice({ tools: { disabled_extensions: ['mcp-crm'] } }, null, crm, true)).toEqual({ disabled: [], disabled_extensions: [], allowed: [] })
  })

  it('asks core for the rows of a catalog, a project and an agent', async () => {
    const client = tauri([])
    await projectTools(client, { builtin: [] }, { disabledTools: ['bash'] }, { allowed: ['bash'] })
    expect(client.invoke).toHaveBeenCalledWith('project_tools', { input: { catalog: { builtin: [] }, policy: { disabledTools: ['bash'] }, agent: { allowed: ['bash'] } } })
    await projectTools(client, null, null)
    expect(client.invoke).toHaveBeenLastCalledWith('project_tools', { input: { catalog: {}, policy: {} } })
  })

  it('names selected skills the machine lacks and the tools of one server', async () => {
    const catalog = await loadToolCatalog(tauri(answer), 'local')
    expect(missingSkills({ skills: [catalog.skills[0].id, 'gone:skills/x/SKILL.md'] }, catalog)).toEqual(['gone:skills/x/SKILL.md'])
    expect(serverTools(catalog, 'Leads')).toEqual({ tools: ['search_leads', 'get_lead'], note: '' })
    expect(serverTools(catalog, 'Calendar').note).toBe('Test this server in Tools to list its tools.')
    expect(serverTools(catalog, 'Unknown')).toEqual({ tools: [], note: '' })
  })
})
