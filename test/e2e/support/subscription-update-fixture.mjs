import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { generateKeyPairSync, randomBytes } from 'node:crypto'
import { signUpdaterBytes } from '../../../.github/lib/updater-signature.mjs'
import { updateFixture } from '../runner/subscription-update.mjs'

// The Rust updater test uses the same HTTPS fixture as installed acceptance.
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-update-client-'))
let fixture
try {
  const bytes = Buffer.from('signed update test package')
  const { privateKey, publicKey } = generateKeyPairSync('ed25519')
  const keyId = randomBytes(8)
  const publicBytes = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32)
  const key = Buffer.concat([Buffer.from('Ed'), keyId, publicBytes]).toString('base64')
  const pubkey = Buffer.from(`untrusted comment: test key\n${key}\n`).toString('base64')
  const signature = signUpdaterBytes(bytes, { keyId, privateKey }, { fileName: 'muniment.msi', version: '1.0.0' })
  fixture = await updateFixture(root, bytes, signature, '1.0.0', 'windows')
  console.log(JSON.stringify({ url: fixture.url, pubkey }))
  // Closing stdin releases the fixture even when a Rust assertion fails.
  await new Promise(resolve => { process.stdin.on('end', resolve); process.stdin.resume() })
} finally {
  await fixture?.close()
  fs.rmSync(root, { recursive: true, force: true })
}
