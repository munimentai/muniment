import { readFileSync } from 'node:fs'
import { expect, it } from 'vitest'

it('runs the installed Windows app without elevation and observes its emitted connection state', () => {
  const workflow = readFileSync('.github/workflows/nightly.yml', 'utf8')
  const smoke = readFileSync('test/e2e/support/windows-installed-smoke.ps1', 'utf8')
  const observer = readFileSync('src-tauri/src/attach_service/commands.rs', 'utf8')
  expect(workflow).toContain('desktop-ci windows --console-user --repo')
  expect(smoke).toContain('The installed smoke requires a non-elevated console session.')
  expect(smoke.indexOf('$null = $app.Handle')).toBeLessThan(smoke.indexOf('$app.HasExited'))
  expect(observer).toMatch(/#\[cfg\(any\(target_os = "macos", target_os = "windows"\)\)\]\s*eprintln!\("desktop runtime client connected=\{connected\}"\)/)
  expect(smoke).toContain("$connected -eq 'desktop runtime client connected=true'")
})
