import fs from 'node:fs'
import path from 'node:path'
import os from 'node:os'
import { spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'
import { acceptance, blocked, platforms } from '../support/subscription-acceptance.mjs'

function redactScreenshot(bytes, name) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'subscription-screenshot-'))
  try {
    const raw = path.join(root, 'raw'), safe = path.join(root, 'safe')
    fs.mkdirSync(raw)
    fs.writeFileSync(path.join(raw, name), bytes, { mode: 0o600 })
    const result = spawnSync(process.execPath, ['test/e2e/support/redact.mjs', raw, safe], {
      stdio: 'ignore', timeout: 10_000,
    })
    if (result.error || result.status !== 0) throw new Error('The screenshot redactor rejected the capture.')
    return fs.readFileSync(path.join(safe, name))
  } finally { fs.rmSync(root, { recursive: true, force: true }) }
}

export function collect(sourceSha, output, inputs) {
  if (!/^[a-f0-9]{40}$/.test(sourceSha)) throw new Error('Provide the exact candidate source SHA.')
  const canonical = directory => fs.existsSync(directory) ? fs.realpathSync(directory) : path.resolve(directory)
  if (inputs.some(input => canonical(input) === canonical(output))) throw new Error('Keep the collection output separate from platform evidence.')
  fs.mkdirSync(output, { recursive: true, mode: 0o700 })
  const combined = { schema: 1, source_sha: sourceSha, packages: {}, cases: [] }
  fs.writeFileSync(path.join(output, 'release-acceptance.json'), JSON.stringify({ ...combined,
    cases: platforms.flatMap(platform => blocked(sourceSha, platform, 'The native evidence collection has not finished.').cases),
  }, null, 2) + '\n', { mode: 0o600 })
  let complete = true
  for (const platform of platforms) {
    const evidenceName = `${platform}-subscription.json`
    const screenshotName = `screenshot-${platform}-subscriptions.png`
    let evidence = { status: 'blocked', reason: 'Run the installed subscription check on this native platform.' }
    let proof = blocked(sourceSha, platform, evidence.reason)
    const matches = inputs.filter(input => fs.existsSync(path.join(input, evidenceName)))
    try {
      if (matches.length !== 1) throw new Error('The platform evidence is missing or duplicated.')
      const input = matches[0]
      const read = name => {
        const file = path.join(input, name)
        if (!fs.lstatSync(file).isFile()) throw new Error('The evidence file is not a regular file.')
        return fs.readFileSync(file)
      }
      evidence = JSON.parse(read(evidenceName))
      const original = JSON.parse(read('release-acceptance.json'))
      if (evidence.status === 'blocked') {
        if (typeof evidence.reason !== 'string' || evidence.reason.length < 20) throw new Error('The blocked reason is missing.')
        proof = blocked(sourceSha, platform, evidence.reason)
        if (JSON.stringify(original) !== JSON.stringify(proof)) throw new Error('The platform proof does not match its runner results.')
        complete = false
        evidence = { status: 'blocked', reason: evidence.reason }
        fs.rmSync(path.join(output, screenshotName), { force: true })
      } else {
        const packages = Object.entries(original.packages ?? {})
        if (packages.length !== 1) throw new Error('The platform proof does not identify one package.')
        const [packageName, sha256] = packages[0]
        const candidate = { source_sha: original.source_sha, platform, sha256 }
        proof = acceptance(candidate, sourceSha, platform, evidence, evidence.transports, packageName)
        if (JSON.stringify(original) !== JSON.stringify(proof)) throw new Error('The platform proof does not match its runner results.')
        const screenshot = redactScreenshot(read(screenshotName), screenshotName)
        if (combined.packages[packageName] && combined.packages[packageName] !== sha256) throw new Error('The package digests conflict.')
        fs.writeFileSync(path.join(output, screenshotName), screenshot, { mode: 0o600 })
        Object.assign(combined.packages, proof.packages)
        evidence = { status: 'passed', installed: true, unchanged: true, webdriver: false,
          source_sha: sourceSha, package_sha256: sha256, models: proof.cases[0].models }
      }
    } catch {
      complete = false
      // Do not copy malformed evidence or untrusted parser error text.
      evidence = { status: 'blocked', reason: 'Run this native platform check again. Supply unique evidence, matching source and package identities, and its screenshot.' }
      proof = blocked(sourceSha, platform, evidence.reason)
      fs.rmSync(path.join(output, screenshotName), { force: true })
    }
    fs.writeFileSync(path.join(output, evidenceName), JSON.stringify(evidence, null, 2) + '\n', { mode: 0o600 })
    combined.cases.push(...proof.cases)
  }
  fs.writeFileSync(path.join(output, 'release-acceptance.json'), JSON.stringify(combined, null, 2) + '\n', { mode: 0o600 })
  return complete ? 0 : 1
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [sourceSha, output, ...inputs] = process.argv.slice(2)
  if (!output) {
    console.error('Use collect-subscriptions.mjs SOURCE_SHA OUTPUT PLATFORM_OUTPUT...')
    process.exitCode = 1
  } else process.exitCode = collect(sourceSha, output, inputs)
}
