// Assembles the search fallback and the question tool the way the per-thread
// agent extension bundles them: the fallback leads the module for its imports,
// and the question tool registers inside the default export.
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'
import { corePath } from '../../../scripts/muniment-core.mjs'

export function agentExtensionSource() {
  const fallback = readFileSync(corePath('crates/core/src/extensions/search-fallback.js'), 'utf8')
  const question = readFileSync(corePath('crates/core/src/extensions/ask-user-question.js'), 'utf8')
  return `${fallback}export default function (pi) {\n  registerSearchFallback(pi);\n${question}\n}\nexport { searchGlob };\n`
}

// Writes the module to its own directory and returns its path and a cleanup.
export function writeAgentExtension() {
  const directory = mkdtempSync(join(tmpdir(), 'muniment-agent-extension-'))
  const file = join(directory, 'agent-extension.mjs')
  writeFileSync(file, agentExtensionSource())
  return { file, cleanup: () => rmSync(directory, { recursive: true, force: true }) }
}

// Loads the module and registers it against a recording stand-in for Pi.
export async function loadAgentExtension() {
  const { file, cleanup } = writeAgentExtension()
  try {
    const module = await import(/* @vite-ignore */ pathToFileURL(file).href)
    const tools = {}
    const handlers = {}
    module.default({ registerTool: tool => { tools[tool.name] = tool }, on: (name, handler) => { handlers[name] = handler } })
    return { module, tools, handlers }
  } finally {
    cleanup()
  }
}
