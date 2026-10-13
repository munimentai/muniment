// A candidate build lives on a draft GitHub release tagged `candidate-<sha>`.
// Drafts are visible only to an authenticated client, and the tag lookup does
// not return them, so the lookup walks the release list.
export const CANDIDATE_TAG_PREFIX = "candidate-";

const FULL_SHA = /^[0-9a-f]{40}$/;
const PER_PAGE = 100;
const MAX_PAGES = 100;

export function candidateTag(sha) {
  if (typeof sha !== "string" || !FULL_SHA.test(sha)) {
    throw new Error(`A candidate release needs a full 40-character lowercase commit SHA, got ${JSON.stringify(sha)}`);
  }
  return `${CANDIDATE_TAG_PREFIX}${sha}`;
}

// Return the single draft release for the commit. A missing, duplicate or
// published match throws, so a caller never falls back to another release.
export async function findCandidateDraft({ github, owner, repo, sha, perPage = PER_PAGE, maxPages = MAX_PAGES }) {
  const tag = candidateTag(sha);
  const matches = [];
  let exhausted = false;
  for (let page = 1; page <= maxPages; page += 1) {
    const { data } = await github.rest.repos.listReleases({ owner, repo, per_page: perPage, page });
    if (!Array.isArray(data)) throw new Error(`Listing releases for ${owner}/${repo} did not return an array`);
    for (const release of data) if (release?.tag_name === tag) matches.push(release);
    if (data.length < perPage) { exhausted = true; break; }
  }
  if (!exhausted) throw new Error(`Gave up looking for ${tag} in ${owner}/${repo} after ${maxPages} pages of releases`);
  if (matches.length === 0) throw new Error(`${owner}/${repo} has no draft release ${tag}`);
  if (matches.length > 1) throw new Error(`${owner}/${repo} has ${matches.length} releases tagged ${tag}`);
  const [release] = matches;
  if (release.draft !== true) throw new Error(`Release ${tag} in ${owner}/${repo} is not a draft`);
  return release;
}
