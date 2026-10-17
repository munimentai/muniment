import fs from 'node:fs'
import path from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { agentTools, emptyCatalog, loadToolCatalog, missingSkills, serverTools, toggleTool } from './agent-tools.js'

const fixture = (name) => JSON.parse(fs.readFileSync(path.join(process.cwd(), 'protocol-fixtures/muniment.attach/1', name), 'utf8')).body
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

  it('lists the catalog\'s tools and any id the agent already turns off', async () => {
    const catalog = await loadToolCatalog(tauri(answer), 'local')
    const tools = agentTools({ tools: { disabled: ['bash', 'mcp__crm__delete_lead'] } }, catalog)
    expect(tools.map((tool) => tool.id)).toEqual(['read', 'write', 'bash', 'web_search', 'mcp__crm__delete_lead'])
    expect(tools.at(-1)).toMatchObject({ known: false, label: 'mcp__crm__delete_lead' })
    expect(agentTools({}, emptyCatalog())).toEqual([])
  })

  it('turns one tool on or off and keeps the rest', () => {
    const agent = { tools: { disabled: ['powershell'] } }
    expect(toggleTool(agent, 'bash', false)).toEqual({ disabled: ['powershell', 'bash'] })
    expect(toggleTool(agent, 'powershell', true)).toEqual({ disabled: [] })
    expect(toggleTool({}, 'bash', false)).toEqual({ disabled: ['bash'] })
  })

  it('names selected skills the machine lacks and the tools of one server', async () => {
    const catalog = await loadToolCatalog(tauri(answer), 'local')
    expect(missingSkills({ skills: [catalog.skills[0].id, 'gone:skills/x/SKILL.md'] }, catalog)).toEqual(['gone:skills/x/SKILL.md'])
    expect(serverTools(catalog, 'Leads')).toEqual({ tools: ['search_leads', 'get_lead'], note: '' })
    expect(serverTools(catalog, 'Calendar').note).toBe('Test this server in Extend to list its tools.')
    expect(serverTools(catalog, 'Unknown')).toEqual({ tools: [], note: '' })
  })
})
