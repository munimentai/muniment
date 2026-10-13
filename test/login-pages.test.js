// @vitest-environment node
import { createServer, ServerResponse } from 'node:http'
import { once } from 'node:events'
import { pathToFileURL } from 'node:url'
import { expect, it } from 'vitest'
import { corePath } from '../scripts/muniment-core.mjs'
import { graphSvg } from '../src/lib/graph-mark.js'

// The runtime's sign-in extension lives in muniment-core beside the Pi adapter.
const { installLoginPages } = await import(pathToFileURL(corePath('crates/core/src/engine/muniment_login.mjs')).href)

it('brands callback pages without changing validation, status, or other responses', async () => {
  const original = ServerResponse.prototype.end
  const success = `<!doctype html><title>Muniment</title>${graphSvg(56)}Signed in`
  const failure = '<!doctype html><title>Muniment</title>Sign-in did not finish'
  const restore = installLoginPages(success, failure)
  const seen = []
  const server = createServer((req, res) => {
    seen.push(req.url)
    const failed = req.url.includes('state=wrong')
    res.writeHead(failed ? 400 : 200, { 'content-type': 'text/html; charset=utf-8' })
    res.end('<!DOCTYPE html>Provider-owned page')
  })
  try {
    server.listen(0, '127.0.0.1')
    await once(server, 'listening')
    const base = `http://127.0.0.1:${server.address().port}`
    const accepted = await fetch(`${base}/callback?code=fixture&state=valid`)
    expect(accepted.status).toBe(200)
    expect(await accepted.text()).toBe(success)
    const rejected = await fetch(`${base}/callback?code=fixture&state=wrong`)
    expect(rejected.status).toBe(400)
    expect(await rejected.text()).toBe(failure)
    expect(await (await fetch(`${base}/other`)).text()).toBe('<!DOCTYPE html>Provider-owned page')
    expect(seen).toEqual(['/callback?code=fixture&state=valid', '/callback?code=fixture&state=wrong', '/other'])
  } finally {
    restore()
    await new Promise(resolve => server.close(resolve))
  }
  expect(ServerResponse.prototype.end).toBe(original)
})
