import fs from 'node:fs/promises'
import path from 'node:path'

const appBinary = process.env.MUNIMENT_E2E_APP_BINARY
const artifactDir = process.env.MUNIMENT_E2E_RAW_DIR
const reportName = process.env.MUNIMENT_E2E_CLEANUP_ONLY === '1' ? 'cleanup' : process.env.MUNIMENT_E2E_ONBOARDING_ONLY === '1' ? 'onboarding' : null
let specName = 'unknown'

if (!appBinary || !path.isAbsolute(appBinary)) throw new Error('MUNIMENT_E2E_APP_BINARY must be an absolute path')
if (!artifactDir || !path.isAbsolute(artifactDir)) throw new Error('MUNIMENT_E2E_RAW_DIR must be an absolute path')

export function redactPageSource(source, values = [process.env.MUNIMENT_E2E_USERNAME, process.env.MUNIMENT_E2E_PASSWORD]) {
  return values.filter(Boolean).sort((left, right) => right.length - left.length)
    .reduce((redacted, value) => redacted.split(value).join('[REDACTED]'), source)
}

export async function captureFailureArtifacts(result, capture = {
  getPageSource: () => browser.getPageSource(),
  saveScreenshot: (destination) => browser.saveScreenshot(destination),
  writeFile: (destination, contents) => fs.writeFile(destination, contents, { mode: 0o600 }),
  log: (message, error) => console.error(message, error),
}) {
  if (result?.passed !== false) return
  const logFailure = (message, error) => {
    try {
      capture.log(message, error)
    } catch {}
  }
  try {
    const source = redactPageSource(await capture.getPageSource())
    await capture.writeFile(path.join(artifactDir, `page-source-${specName}.html`), source)
  } catch (error) {
    logFailure('Failed to capture the page source.', error)
  }
  try {
    await capture.saveScreenshot(path.join(artifactDir, `screenshot-${specName}.png`))
  } catch (error) {
    logFailure('Failed to capture the screenshot.', error)
  }
}

export const config = {
  runner: 'local',
  // Keep local mode first. Mocha records all spec failures because bail stays disabled.
  specs: process.env.MUNIMENT_E2E_CLEANUP_ONLY === '1'
    ? ['./specs/cleanup.spec.js']
    : process.env.MUNIMENT_E2E_ONBOARDING_ONLY === '1'
      ? ['./specs/onboarding.spec.js']
      : ['./specs/local-mode-chat.spec.js', './specs/real-sign-in.spec.js'],
  bail: 0,
  before: (_capabilities, specs) => {
    specName = path.basename(specs[0], '.spec.js').replace(/[^A-Za-z0-9._-]/g, '_')
  },
  maxInstances: 1,
  capabilities: [{ browserName: 'tauri' }],
  logLevel: 'info',
  outputDir: artifactDir,
  framework: 'mocha',
  reporters: [
    ['spec', { addConsoleLogs: true }],
    ['junit', {
      outputDir: artifactDir,
      outputFileFormat: ({ cid, specs }) => {
        const suite = reportName ?? path.basename(specs[0], '.spec.js')
        return `junit-${suite}-${cid}.xml`
      },
    }],
  ],
  mochaOpts: { timeout: 180000 },
  waitforTimeout: 30000,
  services: [['@wdio/tauri-service', {
    appBinaryPath: appBinary,
    driverProvider: 'embedded',
    captureFrontendLogs: true,
    captureBackendLogs: true,
  }]],
  afterTest: async (_test, _context, result) => {
    try {
      await captureFailureArtifacts(result)
    } catch (error) {
      try {
        console.error('Failed to run failure artifact capture.', error)
      } catch {}
    }
  },
}
