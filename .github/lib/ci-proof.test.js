import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { upload } from './artifact-store.mjs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { proofConfiguration, readProof, reusePullRequest, reuseNightlyBuild } from './ci-proof.mjs'

const toolchain = `sha256:${'1'.repeat(64)}`
beforeEach(() => vi.stubEnv('CI_PROOF_CONFIGURATION', JSON.stringify({ CI_TOOLCHAIN_REVISION: toolchain })))
afterEach(() => vi.unstubAllEnvs())

function fixture() {
  const context = { repo: { owner: 'owner', repo: 'repo' }, sha: 'merge', ref: 'refs/heads/main', eventName: 'push', runId: 2 }
  const pull = { merged_at: 'now', merge_commit_sha: 'merge', base: { ref: 'main' }, head: { sha: 'head', repo: { full_name: 'owner/repo' } } }
  const run = { id: 1, head_sha: 'head', event: 'pull_request', path: '.github/workflows/ci.yml', status: 'completed', conclusion: 'success', html_url: 'https://github.com/owner/repo/actions/runs/1', repository: { full_name: 'owner/repo' }, head_repository: { full_name: 'owner/repo' } }
  const proof = { schema: 1, repository: 'owner/repo', run: 1, workflow: 'ci.yml', head: 'head', tree: 'tested-tree', configuration: proofConfiguration() }
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
    ['missing configuration', f => { delete f.proof.configuration }],
    ['different platform configuration', f => { f.configuration = 'a'.repeat(64) }],
    ['different feature configuration', f => { f.configuration = 'b'.repeat(64) }],
    ['different toolchain configuration', f => { f.configuration = 'c'.repeat(64) }],
    ['invalid configuration', f => { f.configuration = f.proof.configuration = '' }],
    ['failed checks', f => { f.run.conclusion = 'failure' }],
    ['pending rerun', f => { f.run.status = 'in_progress' }],
    ['wrong workflow run', f => { f.run.path = '.github/workflows/other.yml' }],
    ['wrong run event', f => { f.run.event = 'push' }],
    ['wrong run head', f => { f.run.head_sha = 'old' }],
    ['unmerged PR', f => { f.pull.merged_at = null }],
    ['different merge commit', f => { f.pull.merge_commit_sha = 'earlier-merge' }],
    ['fork', f => { f.pull.head.repo.full_name = 'someone/fork' }],
    ['untrusted run repository', f => { f.run.repository.full_name = 'someone/fork' }],
    ['untrusted run head', f => { f.run.head_repository.full_name = 'someone/fork' }],
    ['missing run repository', f => { delete f.run.repository }],
    ['missing run head repository', f => { delete f.run.head_repository }],
    ['different base', f => { f.pull.base.ref = 'release' }],
    ['direct push', f => { f.github.rest.repos.listPullRequestsAssociatedWithCommit.mockResolvedValue({ data: [] }) }],
    ['missing or expired proof', f => { f.read.mockResolvedValue(null) }],
    ['artifact service failure', f => { f.read.mockRejectedValue(new Error('unavailable')) }],
    ['PR event', f => { f.context.eventName = 'pull_request' }],
    ['non-main push', f => { f.context.ref = 'refs/heads/repair' }],
    ['cancelled checks', f => { f.run.conclusion = 'cancelled' }],
    ['no workflow run', f => { f.github.rest.actions.listWorkflowRuns.mockResolvedValue({ data: { workflow_runs: [] } }) }],
  ])('runs checks for %s', async (_, change) => {
    const f = fixture()
    change(f)
    expect(await reusePullRequest(f)).toBe(false)
  })
  it('reuses the secret scan only with its own matching proof', async () => {
    const f = fixture()
    f.workflow = f.proof.workflow = 'secret-scan.yml'
    f.run.path = '.github/workflows/secret-scan.yml'
    expect(await reusePullRequest(f)).toBe(true)
  })
  it.each([
    { PLATFORM: 'windows' },
    { VITE_MUNIMENT_CLOUD: 'true' },
    { VITE_MUNIMENT_COMPANY_RECORD: 'true' },
    { VITE_MUNIMENT_CLOUD: 'true', VITE_MUNIMENT_COMPANY_RECORD: 'true' },
    { CI_TOOLCHAIN_REVISION: `sha256:${'2'.repeat(64)}` },
  ])('rejects changed off-tree settings: %j', async settings => {
    const f = fixture()
    expect(await reusePullRequest(f)).toBe(true)
    vi.stubEnv('CI_PROOF_CONFIGURATION', JSON.stringify({ CI_TOOLCHAIN_REVISION: toolchain, ...settings }))
    expect(proofConfiguration()).toMatch(/^[0-9a-f]{64}$/)
    expect(await reusePullRequest(f)).toBe(false)
  })
  it.each([
    undefined, '', '{', 'null', '[]', 'true', '42', '"settings"', '{}',
    JSON.stringify({ PLATFORM: 'linux', VITE_MUNIMENT_CLOUD: 'false' }),
    ...[null, false, 1, {}, [], '', ' ', '2', 'sha256:', `sha256:${'a'.repeat(63)}`,
      `sha256:${'a'.repeat(65)}`, `sha256:${'g'.repeat(64)}`, `${toolchain}\n`, ` ${toolchain}`,
    ].map(identity => JSON.stringify({ CI_TOOLCHAIN_REVISION: identity })),
  ])('runs checks without a valid toolchain identity: %j', async configuration => {
    const f = fixture()
    vi.stubEnv('CI_PROOF_CONFIGURATION', configuration)
    expect(proofConfiguration()).toBeNull()
    expect(await reusePullRequest(f)).toBe(false)
    // Matching missing identities must not turn into reusable proofs.
    f.proof.configuration = proofConfiguration()
    expect(await reusePullRequest(f)).toBe(false)
    expect(f.github.rest.repos.listPullRequestsAssociatedWithCommit).not.toHaveBeenCalled()
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
  const binaries = ['linux-app.deb', 'linux-app.AppImage', 'windows-app.msi', 'windows-app-machine.msi', 'windows-app-nsis.exe', ...['', '-arm64', '-x64'].flatMap(arch => ['.app.zip', '.pkg', '.dmg', '.app.tar.gz'].map(format => `macos-muniment${arch}${format}`))]
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

it('reads bounded MinIO proofs for the exact repository, run, attempt and source', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'ci-proof-test-'))
  const id = { repository: 'munimentai/muniment', run: 1, attempt: 2, source: 'a'.repeat(40) }
  const repo = { owner: 'munimentai', repo: 'muniment' }
  const run = { id: 1, run_attempt: 2, head_sha: id.source }
  const proof = { schema: 1, repository: id.repository, run: 1, attempt: 2, head: id.source, workflow: 'ci.yml' }
  const objects = new Map()
  const store = { put: (key, bytes) => objects.set(key, bytes), get: (key, limit) => {
    const bytes = objects.get(key)
    if (!bytes || bytes.length > limit) throw new Error('Invalid object.')
    return bytes
  } }
  const read = (value = run, name = 'ci.yml-proof') => readProof({}, repo, value, name, store)
  try {
    const save = value => {
      writeFileSync(join(dir, 'proof.json'), JSON.stringify(value))
      upload(id, 'ci.yml-proof', dir, store)
    }
    save(proof)
    expect(await read()).toEqual(proof)
    for (const changed of [{ id: 5 }, { run_attempt: 3 }, { head_sha: 'b'.repeat(40) }]) {
      await expect(read({ ...run, ...changed })).rejects.toThrow()
    }
    await expect(read(run, 'missing')).rejects.toThrow()
    await expect(readProof({}, { owner: 'other', repo: 'muniment' }, run, 'ci.yml-proof', store)).rejects.toThrow()
    for (const changed of [{ schema: 0 }, { run: 5 }, { attempt: 1 }, { head: 'b'.repeat(40) }, { repository: 'other/repo' }]) {
      save({ ...proof, ...changed })
      expect(await read()).toBeNull()
    }
    save({ ...proof, padding: 'x'.repeat(64 * 1024) })
    await expect(read()).rejects.toThrow()
    save(proof)
    objects.set('s3://factory-ci-artifacts/muniment-desktop/1/ci.yml-proof/proof.json', Buffer.from('{}'))
    await expect(read()).rejects.toThrow()
  } finally { rmSync(dir, { recursive: true, force: true }) }
})
