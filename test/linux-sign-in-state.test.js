import { afterEach, describe, expect, it } from 'vitest'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

const temporary = []
afterEach(() => {
  for (const directory of temporary.splice(0)) fs.rmSync(directory, { recursive: true, force: true })
})

describe.skipIf(process.platform === 'win32')('Linux sign-in state cleanup', () => {
  const runner = fs.readFileSync('test/e2e/runner/linux.sh', 'utf8')
  const phases = runner.slice(runner.indexOf('collect_local_mode_pi_log()'), runner.indexOf('\n# shellcheck source=../support/runner-failure.sh')) + runner.slice(runner.indexOf('\nready=1\n'))

  it.each(['absent', 'stale', 'failed-chat', 'cleanup-error'])('handles a %s marker before sign-in', (scenario) => {
    const state = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-sign-in-state-'))
    temporary.push(state)
    const result = spawnSync('bash', ['-c', `
      set -uo pipefail
      state_root=$1
      raw=$1
      cleanup_log="$raw/cleanup.log"
      scenario=$2
      status=0
      trap 'exit "$status"' EXIT
      runner_failure() { status=1; }
      run_e2e() {
        case \${3:-} in
          *local-mode-chat.spec.js)
            [[ $XDG_CONFIG_HOME == "$state_root/ready/config" ]] || exit 2
            printf 'local\\n'
            mkdir -p "$XDG_CONFIG_HOME/ai.muniment.desktop"
            case $scenario in
              stale|failed-chat) touch "$XDG_CONFIG_HOME/ai.muniment.desktop/local-mode" ;;
              cleanup-error) mkdir "$XDG_CONFIG_HOME/ai.muniment.desktop/local-mode" ;;
            esac
            [[ $scenario != failed-chat ]]
            ;;
          *real-sign-in.spec.js)
            [[ $XDG_CONFIG_HOME == "$state_root/ready/config" ]] || exit 2
            [[ ! -e $XDG_CONFIG_HOME/ai.muniment.desktop/local-mode ]] || exit 2
            printf 'sign-in\\n'
            ;;
          *)
            [[ $MUNIMENT_E2E_ONBOARDING_ONLY == 1 ]] || exit 2
            [[ $XDG_CONFIG_HOME != "$state_root/ready/config" ]] || exit 2
            printf 'onboarding\\n'
            ;;
        esac
      }
      ${phases}
    `, 'bash', state, scenario], { encoding: 'utf8' })
    expect(result.status).toBe(['failed-chat', 'cleanup-error'].includes(scenario) ? 1 : 0)
    expect(result.stdout.trim().split('\n')).toEqual(scenario === 'cleanup-error'
      ? ['local', 'onboarding']
      : ['local', 'sign-in', 'onboarding'])
  })
})
