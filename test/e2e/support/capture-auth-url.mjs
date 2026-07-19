import fs from 'node:fs'

const [destination, candidate] = process.argv.slice(2)
if (!destination) throw new Error('auth URL destination is unavailable')
if (/[\u0000-\u001f\u007f]/.test(candidate || '')) throw new Error('invalid auth URL')

let url
try {
  url = new URL(candidate)
} catch {
  throw new Error('invalid auth URL')
}
if (url.protocol !== 'https:' || url.username || url.password) throw new Error('invalid auth URL')

fs.writeFileSync(destination, candidate, { encoding: 'utf8', mode: 0o600 })
