import fs from 'node:fs'
import { pathToFileURL } from 'node:url'

// Type-2 AppImages append SquashFS after the ELF section data and table.
// Read the ELF instead of executing the artifact's --appimage-offset command.
export function appimageOffset(bytes) {
  if (bytes.length < 64 || bytes.subarray(0, 6).toString('hex') !== '7f454c460201' ||
      bytes.subarray(8, 11).toString('hex') !== '414902') {
    throw new Error('Expected a 64-bit little-endian type-2 AppImage')
  }
  const bounded = (value) => {
    if (value > BigInt(bytes.length)) throw new Error('AppImage ELF extent exceeds file size')
    return Number(value)
  }
  const table = bounded(bytes.readBigUInt64LE(40))
  const entrySize = bytes.readUInt16LE(58)
  const count = bytes.readUInt16LE(60)
  if (!count || entrySize < 64 || table < 64) throw new Error('Invalid AppImage section table')
  let end = bounded(BigInt(table) + BigInt(entrySize) * BigInt(count))
  for (let index = 0; index < count; index++) {
    const entry = table + index * entrySize
    if (bytes.readUInt32LE(entry + 4) === 8) continue // SHT_NOBITS has no file payload.
    end = Math.max(end, bounded(bytes.readBigUInt64LE(entry + 24) + bytes.readBigUInt64LE(entry + 32)))
  }
  if (bytes.subarray(end, end + 4).toString() !== 'hsqs') throw new Error('AppImage SquashFS payload is missing')
  return end
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  console.log(appimageOffset(fs.readFileSync(process.argv[2])))
}
