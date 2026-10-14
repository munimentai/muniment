import { setTimeout as sleep } from "node:timers/promises";

// A small GitHub client with the Octokit method shapes the release helpers use.
// Release jobs run on Forgejo, where the job token and API URL belong to
// Forgejo, so they reach GitHub Releases with their own token through this.
export const GITHUB_API = "https://api.github.com";
const UPLOADS = "https://uploads.github.com";

// Wait before each repeat of a call that failed with a network error, a 5xx, or a 429.
export const RETRY_DELAYS = [2000, 5000, 10000];
const transient = (status) => status >= 500 || status === 429;

export function githubReleases(token, fetchImpl = fetch, { delays = RETRY_DELAYS, wait = sleep, log = console.warn } = {}) {
  if (!token) throw new Error("A GitHub release token is required");
  // A POST repeats only with a beforeRetry step that removes what a failed
  // attempt may have created. Other methods repeat as they are.
  const request = async (url, { method = "GET", headers = {}, body, beforeRetry } = {}) => {
    const merged = new Headers({ Accept: "application/vnd.github+json", "X-GitHub-Api-Version": "2022-11-28" });
    for (const [name, value] of Object.entries(headers)) merged.set(name, value);
    merged.set("Authorization", `Bearer ${token}`);
    const retries = method !== "POST" || beforeRetry ? delays : [];
    for (let attempt = 0; ; attempt += 1) {
      let response;
      let failure;
      try {
        response = await fetchImpl(url, { method, headers: merged, body });
      } catch (error) {
        failure = error;
      }
      if (response && !transient(response.status)) {
        if (response.ok) return response;
        // A failed DELETE may have removed the asset before the repeat.
        if (method === "DELETE" && response.status === 404 && attempt > 0) return response;
        const error = new Error(`${method} ${url}: ${response.status} ${await response.text()}`);
        error.status = response.status;
        throw error;
      }
      const reason = response ? `${response.status} ${await response.text()}` : failure.message;
      if (attempt >= retries.length) {
        if (!response) throw failure;
        const error = new Error(`${method} ${url}: ${reason}`);
        error.status = response.status;
        throw error;
      }
      log(`${method} ${url}: ${reason}; retrying in ${retries[attempt] / 1000}s`);
      await wait(retries[attempt]);
      if (beforeRetry) await beforeRetry();
    }
  };
  const json = async (url, options) => ({ data: await (await request(url, options)).json() });
  const repoApi = ({ owner, repo }) => `${GITHUB_API}/repos/${owner}/${repo}`;
  const send = (method, body) => ({ method, headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const deleteReleaseAsset = async ({ owner, repo, asset_id }) => {
    await request(`${repoApi({ owner, repo })}/releases/assets/${asset_id}`, { method: "DELETE" });
    return { data: undefined };
  };
  // A failed upload can leave a partial asset that holds the name and blocks the next upload.
  const removeAssetNamed = async ({ owner, repo, release_id, name }) => {
    for (let page = 1; ; page += 1) {
      const { data } = await json(`${repoApi({ owner, repo })}/releases/${release_id}/assets?per_page=100&page=${page}`);
      const asset = data.find((candidate) => candidate.name === name);
      if (asset) return deleteReleaseAsset({ owner, repo, asset_id: asset.id });
      if (data.length < 100) return;
    }
  };
  return {
    rest: {
      git: {
        getRef: ({ owner, repo, ref }) => json(`${repoApi({ owner, repo })}/git/ref/${ref}`),
      },
      repos: {
        getCommit: ({ owner, repo, ref }) => json(`${repoApi({ owner, repo })}/commits/${ref}`),
        listReleases: ({ owner, repo, per_page = 100, page = 1 }) => json(`${repoApi({ owner, repo })}/releases?per_page=${per_page}&page=${page}`),
        getReleaseByTag: ({ owner, repo, tag }) => json(`${repoApi({ owner, repo })}/releases/tags/${encodeURIComponent(tag)}`),
        // A repeat after a create that reached GitHub would fail on the existing release, so this call does not repeat.
        createRelease: ({ owner, repo, ...body }) => json(`${repoApi({ owner, repo })}/releases`, send("POST", body)),
        updateRelease: ({ owner, repo, release_id, ...body }) => json(`${repoApi({ owner, repo })}/releases/${release_id}`, send("PATCH", body)),
        getReleaseAsset: async ({ owner, repo, asset_id, headers }) => ({
          data: Buffer.from(await (await request(`${repoApi({ owner, repo })}/releases/assets/${asset_id}`, { headers })).arrayBuffer()),
        }),
        deleteReleaseAsset,
        uploadReleaseAsset: ({ owner, repo, release_id, name, data, headers = {} }) => json(
          `${UPLOADS}/repos/${owner}/${repo}/releases/${release_id}/assets?name=${encodeURIComponent(name)}`,
          {
            method: "POST", headers: { "Content-Type": "application/octet-stream", ...headers }, body: data,
            beforeRetry: () => removeAssetNamed({ owner, repo, release_id, name }),
          },
        ),
      },
    },
  };
}
