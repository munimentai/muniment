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

export const stableAssetName = (name, sha, version) => {
  validatePromotionInputs(sha, version);
  const match = name.match(new RegExp(`^nightly-${sha}-(linux|windows|macos)-muniment(.*)$`));
  if (!match) throw new Error(`Unexpected nightly asset name: ${name}`);
  return `muniment-${version.slice(1)}-${match[1]}${match[2].replace(/^_[0-9]+\.[0-9]+\.[0-9]+/, "")}`;
};

export const releaseBody = (sha, version) => `A free desktop app for your models, tools, and files. No Muniment account required.

## Features

- Connect provider accounts, API keys, or local models.
- Work with projects, chats, files, a terminal, and a browser.
- Add MCP tools, skills, and plugins.
- Use reusable agents, artifacts, memory, and on-device voice.

## Install

Download the package for your platform, then connect a provider and start a thread.

| Platform | Download |
| --- | --- |
| macOS | [Installer (.pkg)](https://github.com/munimentai/muniment/releases/download/${version}/muniment-${version.slice(1)}-macos.pkg) |
| Windows x64 | [Installer (.msi)](https://github.com/munimentai/muniment/releases/download/${version}/muniment-${version.slice(1)}-windows_x64_en-US.msi) |
| Ubuntu / Debian x64 | [Package (.deb)](https://github.com/munimentai/muniment/releases/download/${version}/muniment-${version.slice(1)}-linux.deb) |
| Linux x64 | [AppImage](https://github.com/munimentai/muniment/releases/download/${version}/muniment-${version.slice(1)}-linux_amd64.AppImage) |

- macOS: use the .pkg on macOS 13 or later, on Apple silicon or Intel.
- Windows: use the x64 .msi, or the -nsis.exe installer. The -machine.msi installs for all users.
- Linux: use the amd64 .deb on Ubuntu or Debian, or the .AppImage on an x86_64 desktop with X11 or XWayland.

Read the [installation guide](https://muniment.ai/docs/install/) and [getting started guide](https://muniment.ai/docs/start/).

## Release limits

Model weights are not included. Connect a provider or configure a local model server. Provider charges may apply.
The Linux AppImage requires FUSE 2. On Ubuntu, install the .deb first to configure the Chromium sandbox helper.
Paid cloud services, mobile, and the optional company record are outside this desktop release.

## Verification

These binaries passed installed-app checks on macOS, Windows, and Linux at source commit \`${sha}\`.
Windows installers are signed. ${macosSigningProvenance(sha)}
The versioned files preserve the tested binary bytes. Existing download URLs remain available.
`;

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
    created = await (await request(fetchImpl, token, `${repoApi}/releases`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ tag_name: version, target_commitish: sha, name: version, body: releaseBody(sha, version), draft: true, prerelease: false }) })).json();
    const signatures = new Map();
    const stableAssets = [];
    for (const asset of assets) {
      const source = await request(fetchImpl, token, asset.url, { headers: { Accept: "application/octet-stream" } });
      const bytes = await source.arrayBuffer();
      const name = stableAssetName(asset.name, sha, version);
      stableAssets.push({ ...asset, name });
      if (name.endsWith(".sig")) signatures.set(name, Buffer.from(bytes).toString("utf8"));
      await request(fetchImpl, token, `https://uploads.github.com/repos/${repository}/releases/${created.id}/assets?name=${encodeURIComponent(name)}`, { method: "POST", headers: { "Content-Type": asset.content_type || "application/octet-stream" }, body: bytes });
    }
    const feed = createUpdateFeed({ repository, version, assets: stableAssets, signatures });
    await request(fetchImpl, token, `https://uploads.github.com/repos/${repository}/releases/${created.id}/assets?name=latest.json`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(feed) });
    await request(fetchImpl, token, `${repoApi}/releases/${created.id}`, { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ draft: false, prerelease: false }) });
  } catch (error) {
    if (created) { await request(fetchImpl, token, `${repoApi}/releases/${created.id}`, { method: "DELETE" }).catch(() => {}); await request(fetchImpl, token, `${repoApi}/git/refs/tags/${encodeURIComponent(version)}`, { method: "DELETE" }).catch(() => {}); }
    throw error;
  }
}

if (import.meta.url === `file://${process.argv[1]}`) { const [repository, sha, version] = process.argv.slice(2); const token = process.env.GH_TOKEN; await promoteRelease({ token, repository, sha, version }); console.log(`promoted ${sha} to ${version}`); }
