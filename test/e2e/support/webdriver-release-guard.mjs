import fs from 'node:fs'
import path from 'node:path'

const [expectation, artifact] = process.argv.slice(2)
if (!['absent', 'present'].includes(expectation) || !artifact) {
  console.error('usage: webdriver-release-guard.mjs <absent|present> <artifact>')
  process.exit(2)
}

const markers = [Buffer.from('TAURI_WEBDRIVER_PORT'), Buffer.from('wdio-webdriver:default')]
const containsMarker = (candidate) => {
  const stat = fs.lstatSync(candidate)
  if (stat.isSymbolicLink()) return false
  if (stat.isDirectory()) return fs.readdirSync(candidate).some((entry) => containsMarker(path.join(candidate, entry)))
  if (!stat.isFile()) return false
  const contents = fs.readFileSync(candidate)
  return markers.some((marker) => contents.includes(marker))
}
const found = containsMarker(artifact)
if ((expectation === 'present') !== found) {
  console.error(`wdio-webdriver marker must be ${expectation} in ${artifact}`)
  process.exit(1)
}
