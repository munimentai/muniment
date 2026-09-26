// @vitest-environment node
import { expect, it, vi } from 'vitest'
import { readFileSync } from 'node:fs'
const source = readFileSync('src-tauri/core/src/assistant_identity.mjs', 'utf8').replace(/^import \{ completeSimple \}[^\n]*\n/m, '')
const moduleUrl = `data:text/javascript;base64,${Buffer.from(source).toString('base64')}`
const { installBunStreamReleaseFix } = await import(/* @vite-ignore */ moduleUrl)

it('contains the bundled Bun cleanup fault and preserves other errors', () => {
  class Reader {
    releaseLock() { if (this.failure) throw this.failure; return 'released' }
  }
  const release = vi.spyOn(Reader.prototype, 'releaseLock')
  const runtime = { Bun: { version: '1.3.14' }, ReadableStreamDefaultReader: Reader }
  installBunStreamReleaseFix(runtime)
  const reader = new Reader()
  expect(reader.releaseLock()).toBe('released')
  reader.failure = new TypeError('undefined is not a function')
  expect(() => reader.releaseLock()).not.toThrow()
  expect(release.mock.instances).toEqual([reader, reader])
  for (const error of [new TypeError('pending read requests'), new Error('undefined is not a function')]) {
    reader.failure = error
    expect(() => reader.releaseLock()).toThrow(error)
  }
  const installed = Reader.prototype.releaseLock
  installBunStreamReleaseFix(runtime)
  expect(Reader.prototype.releaseLock).toBe(installed)
})

it('leaves Node and other Bun versions unchanged', () => {
  class Reader { releaseLock() {} }
  const original = Reader.prototype.releaseLock
  for (const Bun of [undefined, { version: '1.3.13' }, { version: '1.3.15' }]) {
    installBunStreamReleaseFix({ Bun, ReadableStreamDefaultReader: Reader })
    expect(Reader.prototype.releaseLock).toBe(original)
  }
})
