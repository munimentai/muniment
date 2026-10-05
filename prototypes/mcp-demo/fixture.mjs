import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js'
import { StdioServerTransport } from '@modelcontextprotocol/sdk/server/stdio.js'
import { z } from 'zod'
import { readFile } from 'node:fs/promises'
import { pathToFileURL } from 'node:url'

export const resourceUri = 'ui://muniment-demo/quote.html'
export function createFixture(html) {
  const server = new McpServer({ name: 'Disposable workshop', version: '1.0.0' })
  server.registerTool('quote', {
    description: 'Quote workshop seats at $25 each.',
    inputSchema: { seats: z.number().int().min(1).max(20), fail: z.boolean().optional() },
    annotations: { readOnlyHint: true, destructiveHint: false, openWorldHint: false },
    _meta: { ui: { resourceUri } },
  }, async ({ seats, fail }) => fail
    ? { isError: true, content: [{ type: 'text', text: 'The fixture rejected this quote. Clear fail and retry.' }] }
    : {
      content: [{ type: 'text', text: `${seats} workshop seats cost $${seats * 25}.` }],
      structuredContent: { seats, total: seats * 25 },
      _meta: { ui: { resourceUri }, fixtureMarker: 'Only the app host receives this metadata.' },
    })
  server.registerResource('Workshop quote', resourceUri, { mimeType: 'text/html;profile=mcp-app' }, async () => ({
    contents: [{ uri: resourceUri, mimeType: 'text/html;profile=mcp-app', text: html,
      _meta: { ui: { csp: { connectDomains: [], resourceDomains: [] } } } }],
  }))
  return server
}
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const server = createFixture(await readFile(process.argv[2], 'utf8'))
  await server.connect(new StdioServerTransport())
}
