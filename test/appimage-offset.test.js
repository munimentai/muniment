import { describe, expect, it } from 'vitest'
import { appimageOffset } from './e2e/support/appimage-offset.mjs'

const fixture = () => {
  const bytes = Buffer.alloc(1024)
  Buffer.from('7f454c460201', 'hex').copy(bytes)
  Buffer.from('414902', 'hex').copy(bytes, 8)
  bytes.writeBigUInt64LE(128n, 40)
  bytes.writeUInt16LE(64, 58)
  bytes.writeUInt16LE(1, 60)
  bytes.writeUInt32LE(1, 132)
  bytes.writeBigUInt64LE(256n, 152)
  bytes.writeBigUInt64LE(256n, 160)
  bytes.write('hsqs', 512)
  return bytes
}

describe('AppImage payload offset', () => {
  it('uses the ELF extent rather than a false magic string in the runtime', () => {
    const bytes = fixture()
    bytes.write('hsqs', 80)
    expect(appimageOffset(bytes)).toBe(512)
  })
  it('includes the section table and ignores sections without file data', () => {
    const bytes = fixture()
    bytes.writeUInt32LE(8, 132)
    bytes.writeBigUInt64LE(999999n, 160)
    bytes.write('hsqs', 192)
    expect(appimageOffset(bytes)).toBe(192)
  })
  it('rejects truncated tables, invalid headers, and missing payloads', () => {
    expect(() => appimageOffset(fixture().subarray(0, 150))).toThrow('extent')
    const badHeader = fixture()
    badHeader[8] = 0
    expect(() => appimageOffset(badHeader)).toThrow('type-2')
    const badPayload = fixture()
    badPayload[512] = 0
    expect(() => appimageOffset(badPayload)).toThrow('payload')
  })
})
