import fs from 'node:fs'
import path from 'node:path'

const tauriRoot = path.resolve('src-tauri')
const releaseConfigs = fs.readdirSync(tauriRoot)
  .filter((name) => /^tauri(?:\.(?:linux|windows|macos|machine))?\.conf\.json$/.test(name))
for (const name of releaseConfigs) {
  const contents = fs.readFileSync(path.join(tauriRoot, name), 'utf8')
  if (contents.includes('wdio-webdriver')) {
    console.error(`${name} grants the wdio-webdriver capability`)
    process.exit(1)
  }
}
const defaultCapability = fs.readFileSync(path.join(tauriRoot, 'capabilities/default.json'), 'utf8')
if (defaultCapability.includes('wdio-webdriver')) {
  console.error('the default capability grants wdio-webdriver access')
  process.exit(1)
}
