// The macOS clone's app process cannot reach the model host, while its shell
// can. This forwards the loopback to that host and records every connection,
// so a spec failure names whether Pi dialed at all.
import net from 'node:net'
import fs from 'node:fs'

const [listenPort, targetHost, targetPort, logPath] = process.argv.slice(2)
const record = (line) => {
  try {
    fs.appendFileSync(logPath, `${new Date().toISOString()} ${line}\n`)
  } catch {}
}

const server = net.createServer((client) => {
  record('accepted a connection')
  const upstream = net.connect(Number(targetPort), targetHost)
  upstream.on('error', (error) => {
    record(`upstream failed: ${error.code ?? error.message}`)
    client.destroy()
  })
  client.on('error', (error) => record(`client failed: ${error.code ?? error.message}`))
  client.pipe(upstream)
  upstream.pipe(client)
})

server.on('error', (error) => {
  record(`listen failed: ${error.code ?? error.message}`)
  process.exit(1)
})

server.listen(Number(listenPort), '127.0.0.1', () => {
  record(`listening on 127.0.0.1:${listenPort} for ${targetHost}:${targetPort}`)
})
