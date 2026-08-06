import fs from 'node:fs/promises'
import path from 'node:path'

const appBinary = process.env.MUNIMENT_E2E_APP_BINARY
const artifactDir = process.env.MUNIMENT_E2E_RAW_DIR
const reportName = process.env.MUNIMENT_E2E_CLEANUP_ONLY === '1' ? 'cleanup' : process.env.MUNIMENT_E2E_ONBOARDING_ONLY === '1' ? 'onboarding' : 'sign-in'
const specName = process.env.MUNIMENT_E2E_CLEANUP_ONLY === '1' ? 'cleanup' : process.env.MUNIMENT_E2E_ONBOARDING_ONLY === '1' ? 'onboarding' : 'real-sign-in'

if (!appBinary || !path.isAbsolute(appBinary)) throw new Error('MUNIMENT_E2E_APP_BINARY must be an absolute path')
if (!artifactDir || !path.isAbsolute(artifactDir)) throw new Error('MUNIMENT_E2E_RAW_DIR must be an absolute path')

export function redactPageSource(source, values = [process.env.MUNIMENT_E2E_USERNAME, process.env.MUNIMENT_E2E_PASSWORD]) {
  return values.filter(Boolean).reduce((redacted, value) => redacted.split(value).join('[REDACTED]'), source)
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
  specs: [process.env.MUNIMENT_E2E_CLEANUP_ONLY === '1' ? './specs/cleanup.spec.js' : process.env.MUNIMENT_E2E_ONBOARDING_ONLY === '1' ? './specs/onboarding.spec.js' : './specs/real-sign-in.spec.js'],
  maxInstances: 1,
  capabilities: [{ browserName: 'tauri' }],
  logLevel: 'info',
  outputDir: artifactDir,
  framework: 'mocha',
  reporters: [
    ['spec', { addConsoleLogs: true }],
    ['junit', {
      outputDir: artifactDir,
      outputFileFormat: ({ cid }) => `junit-${reportName}-${cid}.xml`,
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
