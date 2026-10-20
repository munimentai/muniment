// The tools gallery: one turn that called every kind of tool core describes
// (shipped tools, MCP tools with and without a schema, a tool that sets its
// own presentation, an unknown plugin tool, failed calls, long inputs and
// script calls nested under their scripts),
// opened to each call's details, and a turn waiting at a
// web search gate. `agents.html?state=tool-approvals` shows the gallery's
// gates as agent approvals on Notifications.
import { galleryCalls, galleryGates } from './presentations.js'

const GALLERY = (id) => !/^(probe|trust|preview)-/.test(id)
const at = (seconds) => new Date(Date.parse('2026-10-18T12:00:00Z') + seconds * 1000).toISOString()

// Script calls as core sends them: each names its script with its note, and
// each script takes its calls' kind and counts as none. The second script
// makes two.
function scriptCalls(calls) {
  const like = (id, effectId, extra = {}) => ({ ...structuredClone(calls.find((call) => call.effectId === id)), effectId, ...extra })
  const made = (id, effectId, parentEffectId) => {
    const call = like(id, effectId, { parentEffectId })
    call.presentation.via = 'via script'
    return call
  }
  const script = (effectId, kind) => {
    const call = like('script', effectId, { uncounted: true })
    call.presentation.kind = kind
    return call
  }
  return [
    script('script-search', 'web'), made('web', 'script-search/1', 'script-search'),
    script('script-reads', 'other'), made('read', 'script-reads/1', 'script-reads'), made('grep', 'script-reads/2', 'script-reads'),
  ]
}

export function toolsHistory() {
  const gallery = galleryCalls().filter((call) => GALLERY(call.effectId))
  const calls = [...gallery, ...scriptCalls(gallery)]
  const web = galleryGates().find((gate) => gate.id === 'gate-web')
  return [
    {
      runId: 'tools-run-1',
      sentAt: at(0),
      prompt: 'Show me every kind of tool call.',
      phase: 'complete',
      text: 'Here is each call.',
      receipt: { model: 'anthropic/claude-sonnet-4.5', time: '96.0s', cost_usd: 0.0412, tokens: { input: 21000, output: 2300, total: 23300 }, permission_mode: 'manual' },
      toolActivity: calls,
      resumable: false,
    },
    {
      runId: 'tools-run-2',
      sentAt: at(120),
      prompt: 'Search Hacker News for desktop agents.',
      phase: 'pending-permission',
      text: '',
      receipt: null,
      toolActivity: [],
      pendingPermission: { gateId: 'tools-gate', kind: 'select', title: `${web.summary}? ${web.reason}`, options: ['Allow once', 'Deny'], presentation: web.presentation, rule: web.rule, reason: web.reason },
      resumable: false,
    },
  ]
}

// The gallery's gates as the swarm's pending approvals.
export function toolApprovalItems(template) {
  return galleryGates().filter((gate) => gate.id.startsWith('gate-')).map((gate, index) => {
    const approval = {
      ...template,
      approval_id: `tools-run-${index}:${gate.id}`,
      run_id: `tools-run-${index}`,
      gate_id: gate.id,
      agent_name: gate.agent_name,
      request: { gate_id: gate.id, kind: 'select', title: `${gate.summary}? ${gate.reason}`, options: ['Allow once', 'Deny'] },
      rule: gate.rule,
      summary: gate.summary,
      presentation: gate.presentation,
      reason: gate.reason,
      plan: undefined,
      answers: ['allow_once', 'deny'],
      created_at: at(index * 60),
    }
    return { kind: 'approval', id: approval.approval_id, machine: { id: 'local', name: 'This computer' }, agent_id: approval.agent_id, agent_name: approval.agent_name, approval, detail: approval.summary, at: approval.created_at }
  })
}

// Opens the tools line and every group and call. Opening a disclosure
// changes no node, so the calls open in one step.
export function advanceTools() {
  if (toolsRendered()) return
  const workspace = document.querySelector('.workspace')
  const feedback = workspace?.querySelector('details.action-feedback')
  if (!feedback) return
  feedback.open = true
  for (const entry of feedback.querySelectorAll('.tool-groups details:not(.tool-raw)')) entry.open = true
}

export function toolsRendered() {
  const workspace = document.querySelector('.workspace')
  const feedback = workspace?.querySelector('details.action-feedback[open]')
  if (!feedback) return false
  const details = [...feedback.querySelectorAll('.tool-groups details:not(.tool-raw)')]
  return details.length > 0 && details.every((entry) => entry.open)
    && !!workspace.querySelector('.permission-card .tool-call-card')
}
