// Calls one MCP tool with no model. The runtime names the server, the tool,
// its arguments and an output file in the file MUNIMENT_MCP_POLL names. The
// servers extension registers the server, and a scripted provider answers
// the turn with exactly that tool call, so the call runs through Pi's own MCP
// client and tool pipeline. The result goes to the output file. No model is
// asked anything.
import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { createAssistantMessageEventStream } from '@mariozechner/pi-ai'

// Pi names an MCP tool as below, and shortens a name over 64 characters
// with a hash of the server and tool.
function toolName(server, tool) {
  const name = `mcp__${server}__${tool}`.replace(/[^A-Za-z0-9_]/g, '_')
  if (name.length <= 64) return name
  const hash = createHash('sha256').update(`${server}\u0000${tool}`).digest('hex').slice(0, 8)
  return `${name.slice(0, 64 - hash.length - 1)}_${hash}`
}

export default function (pi) {
  const file = process.env.MUNIMENT_MCP_POLL
  if (!file) return
  const request = JSON.parse(readFileSync(file, 'utf8'))
  const name = toolName(request.server, request.tool)
  let written = false
  pi.on('tool_result', event => {
    if (written || event.toolName !== name) return
    written = true
    writeFileSync(request.output, JSON.stringify({
      isError: !!event.isError,
      content: event.content ?? [],
      structuredContent: event.structuredContent ?? null
    }), { mode: 0o600 })
  })
  const message = (model, content, stopReason) => ({
    role: 'assistant', content, api: model.api, provider: model.provider, model: model.id,
    usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
    stopReason, timestamp: Date.now()
  })
  pi.registerProvider('muniment-mcp-poll', {
    baseUrl: 'http://127.0.0.1',
    apiKey: 'muniment-mcp-poll',
    api: 'muniment-mcp-poll',
    streamSimple: (model, context) => {
      const stream = createAssistantMessageEventStream()
      const answered = context.messages.some(entry => entry.role === 'toolResult')
      queueMicrotask(() => {
        stream.push({ type: 'start', partial: message(model, [], answered ? 'stop' : 'toolUse') })
        if (answered) {
          const done = message(model, [{ type: 'text', text: 'done' }], 'stop')
          stream.push({ type: 'text_start', contentIndex: 0, partial: done })
          stream.push({ type: 'text_end', contentIndex: 0, content: 'done', partial: done })
          stream.push({ type: 'done', reason: 'stop', message: done })
        } else {
          const call = { type: 'toolCall', id: 'muniment-mcp-poll', name, arguments: request.args ?? {} }
          const done = message(model, [call], 'toolUse')
          stream.push({ type: 'toolcall_start', contentIndex: 0, partial: done })
          stream.push({ type: 'toolcall_end', contentIndex: 0, toolCall: call, partial: done })
          stream.push({ type: 'done', reason: 'toolUse', message: done })
        }
        stream.end()
      })
      return stream
    },
    models: [{
      id: 'call', name: 'MCP call', reasoning: false, input: ['text'],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow: 128000, maxTokens: 1024
    }]
  })
}
