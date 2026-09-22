import { createUpdateFeed } from "./update-feed.mjs";
const API = "https://api.github.com";
const request = async (fetchImpl, token, url, options = {}) => {
  const response = await fetchImpl(url, { ...options, headers: { Accept: "application/vnd.github+json", Authorization: `Bearer ${token}`, "X-GitHub-Api-Version": "2022-11-28", ...options.headers } });
  if (!response.ok) { const error = new Error(`${options.method ?? "GET"} ${url}: ${response.status} ${await response.text()}`); error.status = response.status; throw error; }
  return response;
};
const getOrNull = async (fetchImpl, token, url) => { try { return await request(fetchImpl, token, url); } catch (error) { if (error.status === 404) return null; throw error; } };

export const validatePromotionInputs = (sha, version) => {
  if (!/^[0-9a-f]{40}$/.test(sha)) throw new Error("source SHA must be exactly 40 lowercase hexadecimal characters");
  if (!/^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/.test(version)) throw new Error("version must be strict SemVer vMAJOR.MINOR.PATCH");
};

export const expectedNightlyAssets = (assets, sha) => {
  const prefix = `nightly-${sha}-`;
  const specs = [
    ["Linux deb", (n) => n.startsWith(`${prefix}linux-`) && n.endsWith(".deb")],
    ["Linux AppImage", (n) => n.startsWith(`${prefix}linux-`) && n.endsWith(".AppImage")],
    ["Windows per-user MSI", (n) => n.startsWith(`${prefix}windows-`) && n.endsWith(".msi") && !n.endsWith("-machine.msi")],
    ["Windows machine MSI", (n) => n.startsWith(`${prefix}windows-`) && n.endsWith("-machine.msi")],
    ["Windows NSIS", (n) => n.startsWith(`${prefix}windows-`) && n.endsWith("-nsis.exe")],
    ["macOS app", (n) => n.startsWith(`${prefix}macos-`) && n.endsWith(".app.zip")],
    ["macOS package", (n) => n.startsWith(`${prefix}macos-`) && n.endsWith(".pkg")],
  ];
  specs.push(["macOS updater", (n) => n.startsWith(`${prefix}macos-`) && n.endsWith(".app.tar.gz")]);
  const updateBinaries = assets.filter(({ name }) => name.endsWith('.AppImage') || name.endsWith('.app.tar.gz') || name.endsWith('-nsis.exe') || name.endsWith('.msi'));
  for (const { name } of updateBinaries) specs.push([`${name} signature`, (n) => n === `${name}.sig`]);
  if (assets.length !== 13 || specs.length !== 13) throw new Error(`nightly release must contain exactly thirteen assets; found ${assets.length}`);
  for (const [label, matches] of specs) if (assets.filter((asset) => matches(asset.name)).length !== 1) throw new Error(`expected exactly one ${label} asset`);
  return assets;
};

export const releaseBody = (sha) => `Stable desktop release promoted from nightly source \`${sha}\`.\n\nWindows installers are signed. ${macosSigningProvenance(sha)} Model weights are not included.`;

export const windowsSigningProvenance = (sha) => `Windows installers for \`${sha}\` were signed by the nightly workflow.`;
export const macosSigningProvenance = (sha) => `macOS artifacts for \`${sha}\` were signed and notarized by the nightly workflow.`;

export const assertGreenCi = (checkRuns, sha) => {
  // The currently-running promotion job can itself be attached to the selected
  // commit. It is not source CI and cannot be completed before this gate runs.
  const sourceChecks = checkRuns.filter((run) => run.name !== "promote");
  const smoke = sourceChecks.find((run) => run.name === "smoke");
  if (!smoke || smoke.status !== "completed" || smoke.conclusion !== "success" || sourceChecks.some((run) => run.status !== "completed" || !["success", "neutral", "skipped"].includes(run.conclusion))) {
    throw new Error(`CI is not green for ${sha}`);
  }
};

const getAllCheckRuns = async (fetchImpl, token, repoApi, sha) => {
  const checkRuns = [];
  for (let page = 1; ; page += 1) {
    const response = await request(fetchImpl, token, `${repoApi}/commits/${sha}/check-runs?per_page=100&page=${page}`);
    const batch = (await response.json()).check_runs;
    checkRuns.push(...batch);
    if (batch.length < 100) return checkRuns;
  }
};

// A compile check or a targeted nightly cannot establish all-platform runtime proof.
export async function assertInstalledNightly(fetchImpl, token, repoApi, sha) {
  const response = await request(fetchImpl, token, `${repoApi}/actions/workflows/nightly.yml/runs?head_sha=${sha}&status=success&per_page=100`);
  const runs = (await response.json()).workflow_runs;
  const required = ["build (linux)", "build (windows)", "build (macos)", "publish", "linux-e2e", "windows-e2e", "macos-e2e"];
  for (const run of runs) {
    if (run.head_sha !== sha || run.status !== "completed" || run.conclusion !== "success") continue;
    const jobs = [];
    for (let page = 1; ; page += 1) {
      const result = await request(fetchImpl, token, `${repoApi}/actions/runs/${run.id}/jobs?filter=latest&per_page=100&page=${page}`);
      const batch = (await result.json()).jobs;
      jobs.push(...batch);
      if (batch.length < 100) break;
    }
    if (required.every((name) => jobs.some((job) => job.name === name && job.status === "completed" && job.conclusion === "success"))) return;
  }
  throw new Error(`No successful full installed nightly for ${sha}`);
}

export async function promoteRelease({ token, repository, sha, version, fetchImpl = fetch }) {
  validatePromotionInputs(sha, version);
  if (!token || !/^[^/]+\/[^/]+$/.test(repository)) throw new Error("token and owner/repository are required");
  const repoApi = `${API}/repos/${repository}`;
  if (await getOrNull(fetchImpl, token, `${repoApi}/git/ref/tags/${encodeURIComponent(version)}`)) throw new Error(`tag ${version} already exists`);
  if (await getOrNull(fetchImpl, token, `${repoApi}/releases/tags/${encodeURIComponent(version)}`)) throw new Error(`release ${version} already exists`);
  const packageFile = await (await request(fetchImpl, token, `${repoApi}/contents/package.json?ref=${sha}`)).json();
  const packageJson = JSON.parse(Buffer.from(packageFile.content, "base64").toString("utf8"));
  if (packageJson.version !== version.slice(1)) throw new Error(`package.json version ${packageJson.version ?? "missing"} does not match ${version}`);
  const checkRuns = await getAllCheckRuns(fetchImpl, token, repoApi, sha);
  assertGreenCi(checkRuns, sha);
  await assertInstalledNightly(fetchImpl, token, repoApi, sha);
  const nightly = await (await request(fetchImpl, token, `${repoApi}/releases/tags/nightly`)).json();
  if (nightly.draft || !nightly.prerelease || !nightly.body?.includes(sha)) throw new Error(`nightly release is not finalized at ${sha}`);
  if (!nightly.body.includes(windowsSigningProvenance(sha))) throw new Error(`nightly Windows installers are not verified as signed for ${sha}`);
  const macosSigned = nightly.body.includes(macosSigningProvenance(sha));
  if (!macosSigned) throw new Error(`nightly macOS artifacts are not verified as signed and notarized for ${sha}`);
  const assets = expectedNightlyAssets(nightly.assets, sha);
  let created;
  try {
    created = await (await request(fetchImpl, token, `${repoApi}/releases`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ tag_name: version, target_commitish: sha, name: version, body: releaseBody(sha), draft: true, prerelease: false }) })).json();
    const signatures = new Map();
    for (const asset of assets) {
      const source = await request(fetchImpl, token, asset.url, { headers: { Accept: "application/octet-stream" } });
      const bytes = await source.arrayBuffer();
      if (asset.name.endsWith(".sig")) signatures.set(asset.name, Buffer.from(bytes).toString("utf8"));
      await request(fetchImpl, token, `https://uploads.github.com/repos/${repository}/releases/${created.id}/assets?name=${encodeURIComponent(asset.name)}`, { method: "POST", headers: { "Content-Type": asset.content_type || "application/octet-stream" }, body: bytes });
    }
    const feed = createUpdateFeed({ repository, version, assets, signatures });
    await request(fetchImpl, token, `https://uploads.github.com/repos/${repository}/releases/${created.id}/assets?name=latest.json`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(feed) });
    await request(fetchImpl, token, `${repoApi}/releases/${created.id}`, { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ draft: false, prerelease: false }) });
  } catch (error) {
    if (created) { await request(fetchImpl, token, `${repoApi}/releases/${created.id}`, { method: "DELETE" }).catch(() => {}); await request(fetchImpl, token, `${repoApi}/git/refs/tags/${encodeURIComponent(version)}`, { method: "DELETE" }).catch(() => {}); }
    throw error;
  }
}

if (import.meta.url === `file://${process.argv[1]}`) { const [repository, sha, version] = process.argv.slice(2); const token = process.env.GH_TOKEN; await promoteRelease({ token, repository, sha, version }); console.log(`promoted ${sha} to ${version}`); }
