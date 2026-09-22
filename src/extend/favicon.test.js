// @vitest-environment node
import { expect, it, vi } from 'vitest'
import { serviceFavicon } from '../../src-tauri/core/src/extend_favicon.mjs'
it('requests only the origin icon and caches its image data', async () => {
  const fetch = vi.fn(async () => new Response(new Uint8Array([1,2,3]), {headers:{'content-type':'image/x-icon'}}))
  expect(await serviceFavicon('https://service.example/private/mcp?token=secret',fetch)).toBe('data:image/x-icon;base64,AQID')
  expect(String(fetch.mock.calls[0][0])).toBe('https://service.example/favicon.ico')
  expect(fetch.mock.calls[0][1]).toMatchObject({credentials:'omit',redirect:'error'})
})
it('falls back on failed, oversized, or non-image responses', async () => {
  expect(await serviceFavicon('https://example.com', async()=>{throw new Error('offline')})).toBeNull()
  expect(await serviceFavicon('https://example.com', async()=>new Response('<html>'))).toBeNull()
  expect(await serviceFavicon('https://example.com', async()=>new Response(new Uint8Array(131073),{headers:{'content-type':'image/png'}}))).toBeNull()
  const fetch=vi.fn()
  expect(await serviceFavicon('file:///tmp/icon',fetch)).toBeNull()
  expect(fetch).not.toHaveBeenCalled()
})
