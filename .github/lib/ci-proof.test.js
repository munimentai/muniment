import { describe, expect, it, vi } from 'vitest'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { readProof, reusePullRequest, reuseNightlyBuild } from './ci-proof.mjs'

function fixture() {
  const context = { repo: { owner: 'owner', repo: 'repo' }, sha: 'merge', ref: 'refs/heads/main', eventName: 'push', runId: 2 }
  const pull = { merged_at: 'now', merge_commit_sha: 'merge', base: { ref: 'main' }, head: { sha: 'head', repo: { full_name: 'owner/repo' } } }
  const run = { id: 1, head_sha: 'head', event: 'pull_request', path: '.github/workflows/ci.yml', status: 'completed', conclusion: 'success', html_url: 'https://github.com/owner/repo/actions/runs/1' }
  const proof = { schema: 1, repository: 'owner/repo', run: 1, workflow: 'ci.yml', head: 'head', tree: 'tested-tree' }
  const summary = { addRaw: vi.fn().mockReturnThis(), write: vi.fn() }
  const core = { notice: vi.fn(), warning: vi.fn(), summary }
  const github = { rest: {
    repos: { listPullRequestsAssociatedWithCommit: vi.fn(async () => ({ data: [pull] })), getReleaseByTag: vi.fn() },
    actions: { listWorkflowRuns: vi.fn(async () => ({ data: { workflow_runs: [run] } })), listJobsForWorkflowRun: vi.fn() },
  } }
  const read = vi.fn(async () => proof)
  return { context, pull, run, proof, core, github, read, workflow: 'ci.yml', tree: 'tested-tree' }
}

describe('exact tested tree reuse', () => {
  it('reuses a successful same-repository PR and links its proof', async () => {
    const f = fixture()
    expect(await reusePullRequest(f)).toBe(true)
    expect(f.core.summary.write).toHaveBeenCalledOnce()
    expect(f.github.rest.actions.listWorkflowRuns).toHaveBeenCalledWith(expect.objectContaining({ event: 'pull_request', head_sha: 'head', workflow_id: 'ci.yml' }))
  })
  it.each([
    ['changed merge tree', f => { f.tree = 'different-tree' }],
    ['different PR head', f => { f.proof.head = 'older-head' }],
    ['different workflow proof', f => { f.proof.workflow = 'secret-scan.yml' }],
    ['failed checks', f => { f.run.conclusion = 'failure' }],
    ['pending rerun', f => { f.run.status = 'in_progress' }],
    ['wrong workflow run', f => { f.run.path = '.github/workflows/other.yml' }],
    ['wrong run event', f => { f.run.event = 'push' }],
    ['wrong run head', f => { f.run.head_sha = 'old' }],
    ['unmerged PR', f => { f.pull.merged_at = null }],
    ['different merge commit', f => { f.pull.merge_commit_sha = 'earlier-merge' }],
    ['fork', f => { f.pull.head.repo.full_name = 'someone/fork' }],
    ['different base', f => { f.pull.base.ref = 'release' }],
    ['direct push', f => { f.github.rest.repos.listPullRequestsAssociatedWithCommit.mockResolvedValue({ data: [] }) }],
    ['missing or expired proof', f => { f.read.mockResolvedValue(null) }],
    ['artifact service failure', f => { f.read.mockRejectedValue(new Error('unavailable')) }],
    ['PR event', f => { f.context.eventName = 'pull_request' }],
  ])('runs checks for %s', async (_, change) => {
    const f = fixture()
    change(f)
    expect(await reusePullRequest(f)).toBe(false)
  })
  it('does not reuse an old success after a failed rerun', async () => {
    const f = fixture()
    f.github.rest.actions.listWorkflowRuns.mockResolvedValue({ data: { workflow_runs: [{ ...f.run, conclusion: 'failure' }, f.run] } })
    expect(await reusePullRequest(f)).toBe(false)
    expect(f.read).not.toHaveBeenCalled()
  })
})

function nightlyFixture() {
  const f = fixture()
  f.context.eventName = 'schedule'
  f.sha = 'a'.repeat(40)
  Object.assign(f.run, { head_sha: f.sha, path: '.github/workflows/nightly.yml', event: 'schedule' })
  const binaries = ['linux-app.deb', 'linux-app.AppImage', 'windows-app.msi', 'windows-app-machine.msi', 'windows-app-nsis.exe', 'macos-app.app.zip', 'macos-app.pkg', 'macos-app.app.tar.gz']
  const names = [...binaries, ...binaries.filter(n => /(?:AppImage|msi|exe|tar.gz)$/.test(n)).map(n => `${n}.sig`)]
  f.assets = names.map((n, i) => ({ id: i + 1, name: `nightly-${f.sha}-${n}`, size: 100, digest: `sha256:${'b'.repeat(64)}` }))
  f.github.rest.repos.getReleaseByTag.mockImplementation(async () => ({ data: { assets: f.assets } }))
  Object.assign(f.proof, { workflow: 'nightly.yml', head: f.sha, assets: [...f.assets].sort((a, b) => a.name.localeCompare(b.name)).map(a => ({ ...a })) })
  f.jobs = ['build (linux)', 'build (windows)', 'build (macos)', 'publish', 'linux-e2e', 'windows-e2e', 'macos-e2e'].map(name => ({ name, status: 'completed', conclusion: 'success' }))
  f.github.rest.actions.listJobsForWorkflowRun.mockImplementation(async () => ({ data: { jobs: f.jobs } }))
  return f
}

describe('unchanged nightly build reuse', () => {
  it('reuses builds only from a full installed run with unchanged asset identities and bytes', async () => {
    expect(await reuseNightlyBuild(nightlyFixture())).toBe(true)
  })
  it.each([
    ['manual dispatch', f => { f.context.eventName = 'workflow_dispatch' }],
    ['changed signing settings', f => { f.settings = { windowsSigning: 'true' } }],
    ['new source', f => { f.proof.head = 'old' }],
    ['replaced asset', f => { f.assets[0].id = 99 }],
    ['changed digest', f => { f.assets[0].digest = `sha256:${'c'.repeat(64)}` }],
    ['missing digest', f => { delete f.assets[0].digest }],
    ['missing signature', f => { f.assets.pop() }],
    ['partial nightly', f => { f.jobs[0].conclusion = 'skipped' }],
    ['failed installed test', f => { f.jobs[4].conclusion = 'failure' }],
    ['no proof', f => { f.read.mockResolvedValue(null) }],
  ])('executes the nightly for %s', async (_, change) => {
    const f = nightlyFixture()
    change(f)
    expect(await reuseNightlyBuild(f)).toBe(false)
  })
})

it('reads bounded JSON from a workflow artifact and rejects an unrelated run', async () => {
  const f = fixture()
  const dir = mkdtempSync(join(tmpdir(), 'ci-proof-test-'))
  try {
    writeFileSync(join(dir, 'proof.json'), JSON.stringify(f.proof))
    execFileSync('zip', ['-q', 'proof.zip', 'proof.json'], { cwd: dir })
    const bytes = readFileSync(join(dir, 'proof.zip'))
    const actions = f.github.rest.actions
    actions.listWorkflowRunArtifacts = vi.fn(async () => ({ data: { artifacts: [{ id: 7, name: 'ci.yml-proof', expired: false, size_in_bytes: bytes.length }] } }))
    actions.downloadArtifact = vi.fn(async () => ({ data: bytes }))
    expect(await readProof(f.github, f.context.repo, f.run, 'ci.yml-proof')).toEqual(f.proof)
    expect(await readProof(f.github, f.context.repo, { id: 5 }, 'ci.yml-proof')).toBeNull()
    expect(await readProof(f.github, f.context.repo, f.run, 'missing')).toBeNull()
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
