import fs from 'node:fs'
import path from 'node:path'
import https from 'node:https'
import { spawnSync } from 'node:child_process'

// Serve the exact signed candidate and one damaged copy on loopback only.
export async function updateFixture(root, bytes, signature, version) {
  if (!Buffer.isBuffer(bytes) || !bytes.length || typeof signature !== 'string' || !signature.trim() ||
      typeof version !== 'string' || !/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error('Provide package bytes, a signature, and the signed version for the update fixture.')
  }
  const key = path.join(root, 'update-key.pem')
  const cert = path.join(root, 'update-cert.pem')
  const openssl = process.platform === 'win32' ? 'C:\\Program Files\\Git\\usr\\bin\\openssl.exe' : 'openssl'
  const result = spawnSync(openssl, ['req', '-x509', '-newkey', 'rsa:2048', '-nodes', '-keyout', key,
    '-out', cert, '-days', '1', '-subj', '/CN=127.0.0.1'], { stdio: 'ignore', timeout: 30_000 })
  if (result.error || result.status !== 0) throw new Error('Install OpenSSL for the disposable update server.')
  fs.chmodSync(key, 0o600)
  const damaged = Buffer.from(bytes)
  damaged[damaged.length - 1] ^= 1
  const server = https.createServer({ key: fs.readFileSync(key), cert: fs.readFileSync(cert) }, (request, response) => {
    if (request.url === '/manifest') {
      response.setHeader('content-type', 'application/json')
      response.end(JSON.stringify({ version, url: `https://127.0.0.1:${server.address().port}/package`, signature }))
    } else if (request.url === '/package' || request.url === '/tampered') {
      response.end(request.url === '/package' ? bytes : damaged)
    } else { response.writeHead(404); response.end() }
  })
  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(0, '127.0.0.1', resolve)
  })
  return { url: `https://127.0.0.1:${server.address().port}/manifest`, close: () => new Promise(resolve => {
    server.close(resolve)
    server.closeAllConnections()
  }) }
}
