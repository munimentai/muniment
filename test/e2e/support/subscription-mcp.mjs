// A local MCP fixture returns only the disposable probe token.
import readline from 'node:readline'
import fs from 'node:fs'
const token = process.argv[2]
if (!/^MUNIMENT-[a-f0-9]{32}$/.test(token ?? '')) process.exit(1)
for await (const line of readline.createInterface({ input: process.stdin })) {
  const request = JSON.parse(line)
  if (request.id === undefined) continue
  let result
  if (request.method === 'initialize') result = { protocolVersion: request.params.protocolVersion,
    capabilities: { tools: {} }, serverInfo: { name: 'release-acceptance', version: '1.0.0' } }
  else if (request.method === 'tools/list') result = { tools: [{ name: 'acceptance_token',
    description: 'Return the release acceptance token.', inputSchema: { type: 'object', properties: {}, additionalProperties: false } }] }
  else if (request.method === 'tools/call' && request.params.name === 'acceptance_token') {
    if (process.argv[3]) fs.writeFileSync(process.argv[3], JSON.stringify({ token }), { mode: 0o600 })
    result = { content: [{ type: 'text', text: token }] }
  } else if (request.method === 'ping') result = {}
  else {
    process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: request.id, error: { code: -32601, message: 'Unknown method.' } }) + '\n')
    continue
  }
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: request.id, result }) + '\n')
}
