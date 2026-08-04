import { spawn } from 'node:child_process'

const adapter = process.argv[2]
if (!adapter) {
  console.error('usage: probe-installed-adapter.mjs <adapter>')
  process.exit(1)
}

const child = spawn(adapter, [], { stdio: ['pipe', 'pipe', 'inherit'] })
let output = ''
let finished = false

const finish = (error) => {
  if (finished) return
  finished = true
  clearTimeout(timer)
  if (child.exitCode === null) child.kill()
  if (error) console.error(`adapter initialize probe failed: ${error}`)
  process.exit(error ? 1 : 0)
}

const timer = setTimeout(() => finish('response timed out'), 10_000)

child.on('error', (error) => finish(error.message))
child.on('close', (code, signal) => {
  if (!finished) finish(`adapter exited before a valid response (code ${code}, signal ${signal})`)
})

child.stdout.setEncoding('utf8')
child.stdout.on('data', (chunk) => {
  output += chunk
  const newline = output.indexOf('\n')
  if (newline === -1) return

  let response
  try {
    response = JSON.parse(output.slice(0, newline))
  } catch (error) {
    finish(`response was not valid JSON: ${error.message}`)
    return
  }

  if (response.jsonrpc !== '2.0' || response.id !== 1) {
    finish('response did not match the initialize request')
  } else if (response.result?.protocolVersion !== 1) {
    finish('protocol version was not 1')
  } else if (response.result?.agentCapabilities?.loadSession !== true) {
    finish('loadSession was not true')
  } else {
    finish()
  }
})

child.stdin.on('error', (error) => finish(error.message))
child.stdin.end(`${JSON.stringify({
  jsonrpc: '2.0',
  id: 1,
  method: 'initialize',
  params: { protocolVersion: 1, clientCapabilities: {} },
})}\n`)
