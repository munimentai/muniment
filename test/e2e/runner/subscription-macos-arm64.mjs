import { spawnSync } from 'node:child_process'
import { pathToFileURL } from 'node:url'
import { guest } from './subscription-guest.mjs'
import { writeBlocked } from './subscriptions.mjs'

export async function hostedArm64({ sourceSha, output, leases, models, repository, token,
  env = process.env, runtime = process.platform, arch = process.arch, uid = process.getuid?.(),
  execute = spawnSync, invoke = guest }) {
  if (!/^[a-f0-9]{40}$/.test(sourceSha ?? '') || !output) {
    throw new Error('Provide an output directory and the exact candidate source SHA.')
  }
  const platform = 'macos-arm64'
  let reason = 'Use the macos-15 GitHub-hosted ARM64 runner without Rosetta.'
  writeBlocked(output, sourceSha, platform, reason)
  try {
    if (runtime !== 'darwin' || arch !== 'arm64' || env.RUNNER_ARCH !== 'ARM64' ||
        env.RUNNER_OS !== 'macOS' || env.GITHUB_ACTIONS !== 'true' || env.RUNNER_ENVIRONMENT !== 'github-hosted') {
      throw new Error(reason)
    }
    const check = (command, args) => {
      const result = execute(command, args, { encoding: 'utf8', timeout: 10_000, stdio: 'pipe' })
      if (result.error || result.status !== 0) throw new Error(reason)
      return result.stdout.trim()
    }
    if (check('/usr/bin/uname', ['-m']) !== 'arm64') throw new Error(reason)
    reason = 'Use the disposable runner console login with an active native GUI session.'
    // GitHub creates a fresh VM for each job. Never use a shared factory login here.
    if (!Number.isSafeInteger(uid) || uid <= 0 || check('/usr/bin/stat', ['-f', '%u', '/dev/console']) !== String(uid)) {
      throw new Error(reason)
    }
    check('/bin/launchctl', ['print', `gui/${uid}`])
  } catch {
    writeBlocked(output, sourceSha, platform, reason)
    console.error(reason)
    return 1
  }
  // The guest keeps the signed package, payload, source, and disposable profile checks.
  return invoke({ sourceSha, platform, output, leases, models, repository, token })
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exitCode = await hostedArm64({
    sourceSha: process.env.SOURCE_SHA,
    output: process.env.OUTPUT,
    leases: process.env.FACTORY_SUBSCRIPTION_LEASES,
    models: process.env.FACTORY_SUBSCRIPTION_MODELS,
    repository: process.env.REPOSITORY,
    token: process.env.GH_TOKEN,
  })
}
