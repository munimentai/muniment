import { cp, mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawn } from 'node:child_process'

const source = dirname(fileURLToPath(import.meta.url))
const root = await mkdtemp(join(tmpdir(), 'muniment-mcp-demo-'))
const demo = join(root, 'prototypes/mcp-demo')
const mode = process.argv[2] || 'start'
if (!['start', 'test', 'test:browser', 'build'].includes(mode)) throw new Error('Choose start, test, test:browser, or build.')
async function run(command, args, cwd) {
  const child = spawn(command, args, { cwd, stdio: 'inherit', env: { ...process.env, npm_config_cache: join(root, 'npm-cache') } })
  const stop = () => child.kill('SIGTERM')
  process.on('SIGINT', stop)
  process.on('SIGTERM', stop)
  try {
    await new Promise((resolve, reject) => {
      child.on('error', reject)
      child.on('exit', code => code === 0 ? resolve() : reject(new Error(`The demo command exited with code ${code}.`)))
    })
  } finally {
    process.off('SIGINT', stop)
    process.off('SIGTERM', stop)
  }
}
try {
  await cp(source, demo, { recursive: true })
  for (const name of ['src', 'public']) await cp(resolve(source, '../..', name), join(root, name), { recursive: true })
  for (const name of ['package.json', 'package-lock.json']) await cp(join(source, name), join(root, name))
  const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm'
  await run(npm, ['ci', '--ignore-scripts', '--strict-peer-deps', '--no-audit', '--no-fund'], root)
  await run(npm, ['ls', '--all'], root)
  if (mode === 'test:browser') await run(process.execPath, [join(root, 'node_modules/playwright/cli.js'), 'install', 'chromium'], demo)
  if (mode === 'build') await run(process.execPath, [join(root, 'node_modules/vite/bin/vite.js'), 'build'], demo)
  else await run(process.execPath, mode === 'test' ? ['--test', 'demo.test.mjs'] : [mode === 'test:browser' ? 'browser-test.mjs' : 'server.mjs'], demo)
} finally {
  await rm(root, { recursive: true, force: true })
}
