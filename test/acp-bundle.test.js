import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()
const adapterSource = 'target/release/muniment-acp'
const adapterDestination = 'muniment-acp'
const runtimeSource = 'target/release/muniment-runtime'
const runtimeDestination = 'muniment-runtime'

const missingAdapterResource = (config) => (
  config.bundle.resources?.[adapterSource] === adapterDestination ? [] : [adapterSource]
)

const missingRuntimeResource = (config) => (
  config.bundle.resources?.[runtimeSource] === runtimeDestination ? [] : [runtimeSource]
)

describe('ACP adapter bundle', () => {
  it('ships the adapter in the Linux desktop package', () => {
    const file = path.join(root, 'src-tauri/tauri.linux.conf.json')
    const config = JSON.parse(fs.readFileSync(file, 'utf8'))

    expect(missingAdapterResource(config)).toEqual([])
  })

  it('rejects an omitted adapter resource', () => {
    const config = { bundle: { resources: {} } }

    expect(missingAdapterResource(config)).toEqual([adapterSource])
  })

  it('ships the runtime in the Linux desktop package', () => {
    const file = path.join(root, 'src-tauri/tauri.linux.conf.json')
    const config = JSON.parse(fs.readFileSync(file, 'utf8'))

    expect(missingRuntimeResource(config)).toEqual([])
  })

  it('rejects an omitted runtime resource', () => {
    const config = { bundle: { resources: {} } }

    expect(missingRuntimeResource(config)).toEqual([runtimeSource])
  })
})
