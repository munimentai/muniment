import path from 'node:path'

const appBinary = process.env.MUNIMENT_E2E_APP_BINARY
const artifactDir = process.env.MUNIMENT_E2E_RAW_DIR

if (!appBinary || !path.isAbsolute(appBinary)) throw new Error('MUNIMENT_E2E_APP_BINARY must be an absolute path')
if (!artifactDir || !path.isAbsolute(artifactDir)) throw new Error('MUNIMENT_E2E_RAW_DIR must be an absolute path')

export const config = {
  runner: 'local',
  specs: ['./specs/**/*.spec.js'],
  maxInstances: 1,
  capabilities: [{ browserName: 'wry' }],
  logLevel: 'info',
  outputDir: artifactDir,
  framework: 'mocha',
  reporters: [['spec', { addConsoleLogs: true }]],
  mochaOpts: { timeout: 180000 },
  waitforTimeout: 30000,
  services: [['@wdio/tauri-service', {
    appBinaryPath: appBinary,
    driverProvider: 'official',
    tauriDriverPath: process.env.MUNIMENT_E2E_TAURI_DRIVER || 'tauri-driver',
    captureFrontendLogs: true,
    captureBackendLogs: true,
  }]],
  afterTest: async (_test, _context, result) => {
    if (!result.passed) await browser.saveScreenshot(path.join(artifactDir, 'failure-current-window.png'))
  },
}
