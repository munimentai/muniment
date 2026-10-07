// A small GitHub client with the Octokit method shapes the release helpers use.
// Release jobs run on Forgejo, where the job token and API URL belong to
// Forgejo, so they reach GitHub Releases with their own token through this.
export const GITHUB_API = "https://api.github.com";
const UPLOADS = "https://uploads.github.com";

export function githubReleases(token, fetchImpl = fetch) {
  if (!token) throw new Error("A GitHub release token is required");
  const request = async (url, { method = "GET", headers = {}, body } = {}) => {
    const merged = new Headers({ Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" });
    for (const [name, value] of Object.entries(headers)) merged.set(name, value);
    merged.set("Authorization", `Bearer ${token}`);
    const response = await fetchImpl(url, { method, headers: merged, body });
    if (!response.ok) {
      const error = new Error(`${method} ${url}: ${response.status} ${await response.text()}`);
      error.status = response.status;
      throw error;
    }
    return response;
  };
  const json = async (url, options) => ({ data: await (await request(url, options)).json() });
  const repoApi = ({ owner, repo }) => `${GITHUB_API}/repos/${owner}/${repo}`;
  const send = (method, body) => ({ method, headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  return {
    rest: {
      git: {
        getRef: ({ owner, repo, ref }) => json(`${repoApi({ owner, repo })}/git/ref/${ref}`),
      },
      repos: {
        getCommit: ({ owner, repo, ref }) => json(`${repoApi({ owner, repo })}/commits/${ref}`),
        getReleaseByTag: ({ owner, repo, tag }) => json(`${repoApi({ owner, repo })}/releases/tags/${encodeURIComponent(tag)}`),
        createRelease: ({ owner, repo, ...body }) => json(`${repoApi({ owner, repo })}/releases`, send("POST", body)),
        updateRelease: ({ owner, repo, release_id, ...body }) => json(`${repoApi({ owner, repo })}/releases/${release_id}`, send("PATCH", body)),
        getReleaseAsset: async ({ owner, repo, asset_id, headers }) => ({
          data: Buffer.from(await (await request(`${repoApi({ owner, repo })}/releases/assets/${asset_id}`, { headers })).arrayBuffer()),
        }),
        deleteReleaseAsset: async ({ owner, repo, asset_id }) => {
          await request(`${repoApi({ owner, repo })}/releases/assets/${asset_id}`, { method: "DELETE" });
          return { data: undefined };
        },
        uploadReleaseAsset: ({ owner, repo, release_id, name, data, headers = {} }) => json(
          `${UPLOADS}/repos/${owner}/${repo}/releases/${release_id}/assets?name=${encodeURIComponent(name)}`,
          { method: "POST", headers: { "Content-Type": "application/octet-stream", ...headers }, body: data },
        ),
      },
    },
  };
}
