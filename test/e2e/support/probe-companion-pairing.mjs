import { randomBytes } from 'node:crypto'
import net from 'node:net'

const endpoint = process.argv[2]
const clientKind = process.argv[3] ?? 'installed-macos-smoke'
let finished = false
let socket

const finish = (passed) => {
  if (finished) return
  finished = true
  clearTimeout(timer)
  socket?.destroy()
  const output = `companion_pairing_probe=${passed ? 'passed' : 'failed'}\n`
  const stream = passed ? process.stdout : process.stderr
  stream.write(output, () => process.exit(passed ? 0 : 1))
}

const timer = setTimeout(() => finish(false), 5_000)

if (!endpoint) {
  finish(false)
} else {
  socket = net.createConnection(endpoint)
  let response = Buffer.alloc(0)

  socket.on('connect', () => {
    const payload = Buffer.from(JSON.stringify({
      protocol: 'muniment.attach/1',
      client: { kind: clientKind, version: '0.0.1' },
      supported: { min: 1, max: 1 },
      client_nonce: randomBytes(16).toString('hex'),
      authorized_client_id: '018f0000-0000-7000-8000-000000000099',
    }))
    const prefix = Buffer.alloc(4)
    prefix.writeUInt32BE(payload.length)
    socket.write(Buffer.concat([prefix, payload]))
  })

  socket.on('data', (chunk) => {
    response = Buffer.concat([response, chunk])
    if (response.length < 4) return
    const payloadLength = response.readUInt32BE(0)
    if (payloadLength > 1024 * 1024) {
      finish(false)
      return
    }
    const frameLength = 4 + payloadLength
    if (response.length < frameLength) return
    if (response.length !== frameLength) {
      finish(false)
      return
    }

    let welcome
    try {
      welcome = JSON.parse(response.subarray(4).toString('utf8'))
    } catch {
      finish(false)
      return
    }

    const valid = welcome !== null && typeof welcome === 'object' && !Array.isArray(welcome) &&
      welcome.selected === 1 &&
      typeof welcome.desktop_version === 'string' && welcome.desktop_version.length > 0 &&
      welcome.authorization === 'pairing_required' &&
      typeof welcome.server_nonce === 'string' && welcome.server_nonce.length > 0 &&
      typeof welcome.approval_challenge === 'string' && welcome.approval_challenge.length > 0
    finish(valid)
  })

  socket.on('error', () => finish(false))
  socket.on('close', () => finish(false))
}
