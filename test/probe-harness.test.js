import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const source = fs.readFileSync(path.join(process.cwd(), 'test/probe/stub.js'), 'utf8')
const readyMarker = "document.body.dataset.probeReady = ''"

describe('probe harness', () => {
  it('sets the ready marker only after the font set settles', () => {
    expect(source).toMatch(/async function markProbeReady\(\) \{\s*await document\.fonts\.ready\s*document\.body\.dataset\.probeReady = ''\s*\}/)
    expect(source.split(readyMarker)).toHaveLength(2)
  })

  it('reports a started attachment listener', () => {
    expect(source).toMatch(/if \(command === 'attach_listener_status'\) return \{ started: true, failure: null \}/)
  })

  it('rejects an unknown core command with its name', () => {
    expect(source).toMatch(/throw new Error\(`Unknown probe command: \$\{command\}`\)/)
  })
})
