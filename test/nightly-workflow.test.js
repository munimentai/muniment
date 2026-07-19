import { describe, expect, it } from 'vitest'
import fs from 'node:fs'

const workflow = fs.readFileSync('.github/workflows/nightly.yml', 'utf8')
const linuxE2e = workflow.slice(workflow.indexOf('  linux-e2e:'))
const jobCondition = linuxE2e.match(/    if: >-\n((?:      .+\n)+)/)[1].trim().replace(/\n\s*/g, ' ')
const conditionResult = ({ eventName, platform, build, publish }) => {
  const expression = jobCondition
    .replace('always()', 'true')
    .replaceAll('needs.build.result', JSON.stringify(build))
    .replaceAll('needs.publish.result', JSON.stringify(publish))
    .replaceAll('github.event_name', JSON.stringify(eventName))
    .replaceAll('github.event.inputs.platform', JSON.stringify(platform))
  return Function(`"use strict"; return (${expression})`)()
}

describe('nightly Linux E2E workflow', () => {
  it('runs after a successful full-nightly publish', () => {
    expect(conditionResult({ eventName: 'schedule', platform: '', build: 'success', publish: 'success' })).toBe(true)
  })

  it('runs a Linux-only dispatch after its package build and finalizes the release identity', () => {
    expect(conditionResult({ eventName: 'workflow_dispatch', platform: 'linux', build: 'success', publish: 'skipped' })).toBe(true)
    expect(linuxE2e).toContain("if: github.event_name == 'workflow_dispatch' && github.event.inputs.platform == 'linux'")
    expect(linuxE2e).toContain('target_commitish: sha')
    expect(linuxE2e).toContain('ref: "tags/nightly"')
    expect(linuxE2e).toContain('`nightly-${sha}-linux-muniment.deb`')
  })

  it('does not run for another targeted platform or an unsuccessful package build', () => {
    expect(conditionResult({ eventName: 'workflow_dispatch', platform: 'windows', build: 'success', publish: 'skipped' })).toBe(false)
    expect(conditionResult({ eventName: 'workflow_dispatch', platform: 'linux', build: 'failure', publish: 'skipped' })).toBe(false)
  })

  it('uploads only generated JUnit XML under the stable report name', () => {
    const reportStep = linuxE2e.slice(linuxE2e.indexOf('      - name: Upload stable JUnit report'), linuxE2e.indexOf('      - name: Preserve E2E result'))
    expect(reportStep).toContain('if: always()')
    expect(reportStep).toContain('name: linux-e2e-report')
    expect(reportStep).toContain('path: ${{ runner.temp }}/muniment-e2e-artifacts/junit-*.xml')
    expect(reportStep).not.toContain('path: ${{ runner.temp }}/muniment-e2e-artifacts\n')
  })
})
