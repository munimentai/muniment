import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { writeFileSync } from 'node:fs'
import { readArtifact } from './artifact-store.mjs'
import { join } from 'node:path'

// Proofs are data from a successful, same-repository workflow. Never execute
// downloaded content. A missing proof always falls back to running the checks.
const schema = 1
const limit = 64 * 1024
export const checkoutTree = () => execFileSync('git', ['rev-parse', 'HEAD^{tree}'], { encoding: 'utf8' }).trim()

// The tree binds platform, feature and toolchain definitions in the repository.
// Require an off-tree toolchain inventory digest before hashing repository variables.
// Missing or invalid identities disable reuse, not the checks or proof upload.
export function proofConfiguration() {
  const configuration = process.env.CI_PROOF_CONFIGURATION
  try {
    const settings = JSON.parse(configuration)
    if (!settings || Array.isArray(settings) || typeof settings !== 'object' ||
        typeof settings.CI_TOOLCHAIN_REVISION !== 'string' || settings.CI_TOOLCHAIN_REVISION.length !== 71 ||
        !/^sha256:[0-9a-f]{64}$/.test(settings.CI_TOOLCHAIN_REVISION)) return null
    return createHash('sha256').update(configuration).digest('hex')
  } catch {
    return null
  }
}

export function writeProof(context, workflow, extra = {}) {
  const proof = {
    schema, workflow, repository: `${context.repo.owner}/${context.repo.repo}`,
    run: context.runId, attempt: Number(process.env.GITHUB_RUN_ATTEMPT),
    head: context.payload.pull_request?.head.sha ?? context.sha,
    tree: checkoutTree(), configuration: proofConfiguration(), ...extra,
  }
  const file = join(process.env.RUNNER_TEMP, 'proof.json')
  writeFileSync(file, JSON.stringify(proof))
  return file
}

export async function readProof(github, repo, run, name, store) {
  const repository = `${repo.owner}/${repo.repo}`
  const files = readArtifact({ repository, run: run.id, attempt: run.run_attempt, source: run.head_sha }, name, store, limit)
  if (files.size !== 1 || !files.has('proof.json')) return null
  const proof = JSON.parse(files.get('proof.json'))
  return proof.schema === schema && proof.run === run.id && proof.attempt === run.run_attempt &&
    proof.head === run.head_sha && proof.repository === repository ? proof : null
}

export async function reusePullRequest({ github, context, core, workflow, tree = checkoutTree(), configuration = proofConfiguration(), read = readProof }) {
  if (context.eventName !== 'push' || context.ref !== 'refs/heads/main' ||
      typeof configuration !== 'string' || configuration.length !== 64 || !/^[0-9a-f]{64}$/.test(configuration)) return false
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
          run.repository?.full_name !== `${context.repo.owner}/${context.repo.repo}` ||
          run.head_repository?.full_name !== `${context.repo.owner}/${context.repo.repo}` ||
          run.path !== `.github/workflows/${workflow}` || run.status !== 'completed' || run.conclusion !== 'success') continue
      const proof = await read(github, context.repo, run, `${workflow}-proof`)
      if (!proof || proof.workflow !== workflow || proof.head !== pull.head.sha || proof.tree !== tree ||
          proof.configuration !== configuration) continue
      core.notice(`Reuse ${workflow} from ${run.html_url}: the tested Git tree matches ${tree}.`)
      await core.summary.addRaw(`Reused [successful PR checks](${run.html_url}) for Git tree \`${tree}\`.`).write()
      return true
    }
  } catch (error) {
    core.warning(`Cannot reuse ${workflow}: ${error.message}. Running the checks.`)
  }
  return false
}
