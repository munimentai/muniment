// The agent import flow: a share link, a file or pasted text goes to core's
// `agent.import`, which answers a preview. Committing the preview saves the
// agent through `agent.import_commit`. Core reads every format and maps
// routines, skills and tools; the window only shows the preview.
import { agentRequest } from './agent-runtime.js'

const MAX_BYTES = 131072

// The one source the chosen way names: a link, a file's text or pasted text.
export function importSource({ way, link = '', file = '', paste = '' }) {
  const source = way === 'link' ? link.trim() : way === 'file' ? file : paste
  if (!source.trim()) throw new Error(way === 'link' ? 'Paste a share link.' : way === 'file' ? 'Choose a template file.' : 'Paste a template.')
  if (way === 'link' && !source.startsWith('https://')) throw new Error('Paste a link that starts with https://.')
  if (new TextEncoder().encode(source).length > MAX_BYTES) throw new Error('Choose a template under 128 KB.')
  return source
}

export function importPreview(tauri, runtime, source) {
  return agentRequest(tauri, runtime, 'agent.import', { source })
}

// The commit body: the previewed agent, with its memories when kept.
export function commitBody(preview, { memories = true } = {}) {
  return { agent: preview.agent, ...(memories && preview.memories?.length ? { memories: preview.memories } : {}) }
}

export function commitImport(tauri, runtime, preview, options) {
  return agentRequest(tauri, runtime, 'agent.import_commit', commitBody(preview, options))
}

const SOURCES = { grok_link: 'Grok share link', grok_json: 'Grok JSON', markdown: 'Markdown', muniment: 'Muniment agent file' }

// The lines a preview shows above its lists.
export function previewFacts(preview) {
  const routines = (preview?.agent?.routines ?? []).length
  return {
    source: SOURCES[preview?.source] ?? 'Template',
    coverage: preview?.coverage === 'public-profile' ? 'The share page holds the public profile only. Skills, memories and routines are not in it.' : '',
    routines: routines ? `${routines} ${routines === 1 ? 'routine arrives' : 'routines arrive'} off, as proposals to accept.` : 'No routines.',
  }
}

// Reads a chosen file as text for the preview.
export async function readTemplateFile(file) {
  if (!file) return ''
  if (file.size > MAX_BYTES) throw new Error('Choose a template under 128 KB.')
  return typeof file.text === 'function' ? file.text() : new Response(file).text()
}

// The file `agent.export` answers, saved where the user picks.
export async function exportAgent(tauri, agent, format) {
  const file = await agentRequest(tauri, agent.runtimeId, 'agent.export', { id: agent.id, format })
  return tauri.invoke('agent_export_template', { content: file.content, fileName: file.file_name })
}
