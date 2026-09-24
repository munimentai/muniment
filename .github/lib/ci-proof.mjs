import { execFileSync } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { expectedNightlyAssets } from './release-promotion.mjs'

// Proofs are data from a successful, same-repository workflow. Never execute
// downloaded content. A missing proof always falls back to running the checks.
const schema = 1
const limit = 64 * 1024
export const checkoutTree = () => execFileSync('git', ['rev-parse', 'HEAD^{tree}'], { encoding: 'utf8' }).trim()

export function writeProof(context, workflow, extra = {}) {
  const proof = {
    schema, workflow, repository: `${context.repo.owner}/${context.repo.repo}`,
    run: context.runId, head: context.payload.pull_request?.head.sha ?? context.sha,
    tree: checkoutTree(), ...extra,
  }
  const file = join(process.env.RUNNER_TEMP, 'proof.json')
  writeFileSync(file, JSON.stringify(proof))
  return file
}

export async function readProof(github, repo, run, name) {
  const { data } = await github.rest.actions.listWorkflowRunArtifacts({ ...repo, run_id: run.id, per_page: 100 })
  const artifacts = data.artifacts.filter(a => a.name === name && !a.expired)
  if (artifacts.length !== 1 || artifacts[0].size_in_bytes > limit) return null
  const archive = await github.rest.actions.downloadArtifact({ ...repo, artifact_id: artifacts[0].id, archive_format: 'zip' })
  const bytes = Buffer.from(archive.data)
  if (bytes.length > limit) return null
  const dir = mkdtempSync(join(tmpdir(), 'ci-proof-'))
  try {
    const file = join(dir, 'proof.zip')
    writeFileSync(file, bytes)
    const proof = JSON.parse(execFileSync('unzip', ['-p', file, 'proof.json'], { maxBuffer: limit, encoding: 'utf8' }))
    return proof.schema === schema && proof.run === run.id &&
      proof.repository === `${repo.owner}/${repo.repo}` ? proof : null
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
}

export async function reusePullRequest({ github, context, core, workflow, tree = checkoutTree(), read = readProof }) {
  if (context.eventName !== 'push' || context.ref !== 'refs/heads/main') return false
  try {
    const { data: pulls } = await github.rest.repos.listPullRequestsAssociatedWithCommit({ ...context.repo, commit_sha: context.sha })
    for (const pull of pulls) {
      if (!pull.merged_at || pull.merge_commit_sha !== context.sha || pull.base.ref !== 'main' ||
          pull.head.repo?.full_name !== `${context.repo.owner}/${context.repo.repo}`) continue
      const { data } = await github.rest.actions.listWorkflowRuns({
        ...context.repo, workflow_id: workflow, event: 'pull_request', head_sha: pull.head.sha, per_page: 100,
      })
      // Do not reuse an older success after a failed or pending rerun.
      const run = data.workflow_runs[0]
      if (!run || run.event !== 'pull_request' || run.head_sha !== pull.head.sha ||
          run.path !== `.github/workflows/${workflow}` || run.status !== 'completed' || run.conclusion !== 'success') continue
      const proof = await read(github, context.repo, run, `${workflow}-proof`)
      if (!proof || proof.workflow !== workflow || proof.head !== pull.head.sha || proof.tree !== tree) continue
      core.notice(`Reuse ${workflow} from ${run.html_url}: the tested Git tree matches ${tree}.`)
      await core.summary.addRaw(`Reused [successful PR checks](${run.html_url}) for Git tree \`${tree}\`.`).write()
      return true
    }
  } catch (error) {
    core.warning(`Cannot reuse ${workflow}: ${error.message}. Running the checks.`)
  }
  return false
}

export async function nightlyAssets(github, repo, sha) {
  const { data } = await github.rest.repos.getReleaseByTag({ ...repo, tag: 'nightly' })
  return expectedNightlyAssets(data.assets, sha).map(({ id, name, size, digest }) => {
    if (!id || !size || !/^sha256:[0-9a-f]{64}$/.test(digest ?? '')) throw new Error('Missing asset identity or digest')
    return { id, name, size, digest }
  }).sort((a, b) => a.name.localeCompare(b.name))
}

export async function reuseNightly({ github, context, core, sha, settings = {}, read = readProof }) {
  // A manual dispatch always runs, including requests to recheck infrastructure.
  if (context.eventName !== 'schedule') return false
  try {
    const assets = await nightlyAssets(github, context.repo, sha)
    const { data } = await github.rest.actions.listWorkflowRuns({
      ...context.repo, workflow_id: 'nightly.yml', status: 'success', head_sha: sha, per_page: 100,
    })
    for (const run of data.workflow_runs) {
      if (run.id === context.runId || run.head_sha !== sha || run.status !== 'completed' ||
          run.conclusion !== 'success' || run.path !== '.github/workflows/nightly.yml') continue
      const proof = await read(github, context.repo, run, 'nightly.yml-proof')
      if (!proof || proof.workflow !== 'nightly.yml' || proof.head !== sha || JSON.stringify(proof.settings ?? {}) !== JSON.stringify(settings) || JSON.stringify(proof.assets) !== JSON.stringify(assets)) continue
      const { data: { jobs } } = await github.rest.actions.listJobsForWorkflowRun({ ...context.repo, run_id: run.id, filter: 'latest', per_page: 100 })
      const required = ['build (linux)', 'build (windows)', 'build (macos)', 'publish', 'linux-e2e', 'windows-e2e', 'macos-e2e']
      if (!required.every(name => jobs.some(job => job.name === name && job.status === 'completed' && job.conclusion === 'success'))) continue
      core.notice(`Reuse installed nightly ${run.html_url}: source and all thirteen assets match.`)
      await core.summary.addRaw(`Reused [full installed nightly](${run.html_url}). Source \`${sha}\` and all thirteen asset IDs and SHA-256 digests match.`).write()
      return true
    }
  } catch (error) {
    core.warning(`Cannot reuse nightly: ${error.message}. Running the build and installed tests.`)
  }
  return false
}
