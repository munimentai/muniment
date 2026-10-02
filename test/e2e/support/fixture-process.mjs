import { spawnSync } from 'node:child_process'

export const POWERSHELL_TEST_TIMEOUT = 30_000

export function spawnFixture(command, args, options = {}) {
  // Synchronous spawns block the test timer. Give the child the full test budget and keep a kill bound.
  const timeout = options.timeout ?? POWERSHELL_TEST_TIMEOUT
  const result = spawnSync(command, args, { encoding: 'utf8', ...options, timeout })
  if (result.error?.code === 'ETIMEDOUT') {
    throw new Error(`The fixture process timed out after ${timeout} ms: ${command} ${args.join(' ')}\nstdout:\n${result.stdout ?? ''}\nstderr:\n${result.stderr ?? ''}`, { cause: result.error })
  }
  return result
}
