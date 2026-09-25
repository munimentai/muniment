import { spawnSync } from "node:child_process";
import { createHash, X509Certificate } from "node:crypto";
import { closeSync, existsSync, mkdirSync, mkdtempSync, openSync, readdirSync, readFileSync, readSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";
import { createUpdateFeed } from "./update-feed.mjs";
import { verifyUpdaterSignature } from "./updater-signature.mjs";
import { macosArchitectures, macosFormats } from "./macos-variants.mjs";
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
    ...macosArchitectures.flatMap(suffix => macosFormats.map(format => [
      `macOS ${suffix || 'universal'} ${format}`, n => n === `${prefix}macos-muniment${suffix}${format}`,
    ])),
  ];
  const updateBinaries = assets.filter(({ name }) => name.endsWith('.AppImage') || name.endsWith('.app.tar.gz') || name.endsWith('-nsis.exe') || name.endsWith('.msi'));
  for (const { name } of updateBinaries) specs.push([`${name} signature`, (n) => n === `${name}.sig`]);
  if (assets.length !== 24 || specs.length !== 24) throw new Error(`nightly release must contain exactly 24 assets; found ${assets.length}`);
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
| macOS Apple silicon | [Disk image (.dmg)](https://github.com/munimentai/muniment/releases/download/${version}/muniment-${version.slice(1)}-macos-arm64.dmg) |
| macOS Intel | [Disk image (.dmg)](https://github.com/munimentai/muniment/releases/download/${version}/muniment-${version.slice(1)}-macos-x64.dmg) |
| macOS Universal | [Disk image (.dmg)](https://github.com/munimentai/muniment/releases/download/${version}/muniment-${version.slice(1)}-macos.dmg) |
| Windows x64 | [Installer (.msi)](https://github.com/munimentai/muniment/releases/download/${version}/muniment-${version.slice(1)}-windows_x64_en-US.msi) |
| Ubuntu / Debian x64 | [Package (.deb)](https://github.com/munimentai/muniment/releases/download/${version}/muniment-${version.slice(1)}-linux.deb) |
| Linux x64 | [AppImage](https://github.com/munimentai/muniment/releases/download/${version}/muniment-${version.slice(1)}-linux_amd64.AppImage) |

- macOS: open the .dmg and drag muniment to Applications. Choose Universal if you are unsure of your Mac chip. Signed .pkg installers remain available for managed installs. macOS 13 or later is required.
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

// The signers a stable release accepts. A change of signing identity is a
// reviewed change to these constants.
export const WINDOWS_PUBLISHER = "CN=Green Kangaroo\\, LLC,O=Green Kangaroo\\, LLC,L=Murrells Inlet,ST=South Carolina,C=US";
// OpenSSL's legacy display reverses the RDN order and separates fields with
// slashes. Accept only these two exact renderings of the pinned identity.
const WINDOWS_PUBLISHER_OPENSSL = "/C=US/ST=South Carolina/L=Murrells Inlet/O=Green Kangaroo, LLC/CN=Green Kangaroo, LLC";
export const APPLE_TEAM_ID = "VF895CP335";
// Azure Artifact Signing chains to this Microsoft root, which the system CA
// bundle does not carry. The file is pinned by its SHA-256.
export const MICROSOFT_ROOT = ".github/certs/MicrosoftIdentityVerificationRootCA2020.cer";
export const MICROSOFT_ROOT_SHA256 = "5367f20c7ade0e2bca790915056d086b720c33c1fa2a2661acf787e3292e1270";

const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const updateBinary = (name) => name.endsWith(".AppImage") || name.endsWith(".app.tar.gz") || name.endsWith("-nsis.exe") || name.endsWith(".msi");
const tool = (run, command, args) => {
  const result = run(command, args, { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
  return { ok: !result.error && result.status === 0, output: `${result.stdout ?? ""}\n${result.stderr ?? ""}` };
};

export const microsoftRootPem = (der = readFileSync(MICROSOFT_ROOT)) => {
  if (sha256(der) !== MICROSOFT_ROOT_SHA256) throw new Error(`${MICROSOFT_ROOT} does not match its pinned SHA-256`);
  return new X509Certificate(der).toString();
};

// osslsigncode checks the Authenticode digest, the chain to the pinned root at
// the timestamp time, and the timestamp chain. The signer must be the publisher.
export const verifyAuthenticode = (file, caFile, run = spawnSync) => {
  const { ok, output } = tool(run, "osslsigncode", ["verify", "-CAfile", caFile, "-TSA-CAfile", caFile, "-in", file]);
  if (!ok || !/^Succeeded\s*$/m.test(output)) throw new Error(`Authenticode verification failed for ${basename(file)}`);
  const signer = output.slice(output.indexOf("Signer's certificate:")).match(/Subject: ?([^\r\n]+)/)?.[1]?.trim();
  if (output.indexOf("Signer's certificate:") < 0 || ![WINDOWS_PUBLISHER, WINDOWS_PUBLISHER_OPENSSL].includes(signer)) {
    throw new Error(`${basename(file)} is signed by ${signer ?? "an unknown signer"}, not ${WINDOWS_PUBLISHER}`);
  }
};

// rcodesign reports each signature's CMS check and certificate chain. Require a
// valid signature, a chain to the Apple root, and the Developer ID of the team.
const requireAppleSignature = (output, kind, label) => {
  const identity = new RegExp(`CN=Developer ID ${kind}: [^\\r\\n]*\\(${APPLE_TEAM_ID}\\)`);
  if (!identity.test(output)) throw new Error(`${label} is not signed with the Developer ID ${kind} identity of team ${APPLE_TEAM_ID}`);
  if (/chains_to_apple_root_ca: false/.test(output) || !/chains_to_apple_root_ca: true/.test(output)) throw new Error(`${label} does not chain to the Apple root`);
  if (/^\s*signature_verifies: false/m.test(output) || !/^\s*signature_verifies: true/m.test(output)) throw new Error(`${label} carries a signature that does not verify`);
};

const MACHO_MAGIC = new Set(["cffaedfe", "feedfacf", "cefaedfe", "feedface", "cafebabe", "bebafeca"]);
const machOFiles = (directory) => readdirSync(directory, { withFileTypes: true, recursive: true })
  .filter((entry) => entry.isFile())
  .map((entry) => join(entry.parentPath ?? entry.path, entry.name))
  .filter((file) => {
    const descriptor = openSync(file, "r");
    const magic = Buffer.alloc(4);
    try { readSync(descriptor, magic, 0, 4, 0); } finally { closeSync(descriptor); }
    return MACHO_MAGIC.has(magic.toString("hex"));
  });

// Check every Mach-O file in an extracted app, and the stapled ticket. The
// Linux runner has no Gatekeeper, so this checks what rcodesign can check: code
// directory digests, CMS signatures, the certificate chain and the team.
export const verifyMacosApp = (app, run = spawnSync) => {
  if (!existsSync(join(app, "Contents", "CodeResources"))) throw new Error(`${basename(app)} carries no stapled notarization ticket`);
  const files = machOFiles(app);
  if (files.length === 0) throw new Error(`${basename(app)} holds no Mach-O code`);
  for (const file of files) {
    const label = file.slice(app.length - basename(app).length);
    if (!tool(run, "rcodesign", ["verify", file]).ok) throw new Error(`rcodesign rejects the signature of ${label}`);
    const info = tool(run, "rcodesign", ["print-signature-info", file]);
    if (!info.ok) throw new Error(`rcodesign cannot read the signature of ${label}`);
    requireAppleSignature(info.output, "Application", label);
  }
};

export const verifyMacosPackage = (pkg, run = spawnSync) => {
  const info = tool(run, "rcodesign", ["print-signature-info", pkg]);
  if (!info.ok) throw new Error(`rcodesign cannot read the signature of ${basename(pkg)}`);
  // A flat package carries a legacy RSA signature and a CMS signature over its
  // table of contents. Installer validates the CMS one, and rcodesign reports
  // the legacy RSA check as false on packages Apple accepts, so require the
  // table-of-contents checksum and the CMS signature.
  if (!/checksum_verifies: true/.test(info.output) || !/cms_signature_verifies: true/.test(info.output)) {
    throw new Error(`${basename(pkg)} carries a package signature that does not verify`);
  }
  requireAppleSignature(info.output, "Installer", basename(pkg));
};

export const verifyMacosDmg = (dmg, run = spawnSync) => {
  const info = tool(run, "rcodesign", ["print-signature-info", dmg]);
  if (!info.ok) throw new Error(`rcodesign cannot read the signature of ${basename(dmg)}`);
  requireAppleSignature(info.output, "Application", basename(dmg));
};

// Verify the downloaded nightly bytes themselves, not the release text: every
// updater signature against the committed key, Authenticode on the Windows
// installers, and the Apple signatures in both app archives and the package.
export const verifyNightlyArtifacts = ({ files, workDir, publicKey = readFileSync(join("src-tauri", "updater.pub"), "utf8"), run = spawnSync, caPem = microsoftRootPem() }) => {
  const caFile = join(workDir, "microsoft-root.pem");
  writeFileSync(caFile, caPem);
  for (const [name, file] of files) {
    if (updateBinary(name)) {
      const signature = files.get(`${name}.sig`);
      if (!signature) throw new Error(`${name} has no updater signature`);
      try {
        verifyUpdaterSignature(readFileSync(file), readFileSync(signature, "utf8"), publicKey);
      } catch (error) {
        throw new Error(`${name} updater signature: ${error.message}`);
      }
    }
    if (name.endsWith(".msi") || name.endsWith("-nsis.exe")) verifyAuthenticode(file, caFile, run);
    if (name.endsWith(".pkg")) verifyMacosPackage(file, run);
    if (name.endsWith(".dmg")) verifyMacosDmg(file, run);
    if (name.endsWith(".app.zip") || name.endsWith(".app.tar.gz") || name.endsWith(".dmg")) {
      const target = join(workDir, `extracted-${basename(name)}`);
      mkdirSync(target);
      const extracted = name.endsWith(".dmg") ? tool(run, "7z", ["x", "-y", "-x!Muniment/Applications", `-o${target}`, file])
        : name.endsWith(".zip") ? tool(run, "unzip", ["-q", file, "-d", target]) : tool(run, "tar", ["-xzf", file, "-C", target]);
      if (!extracted.ok) throw new Error(`cannot extract ${name}`);
      const apps = name.endsWith(".dmg")
        ? readdirSync(target, { recursive: true, withFileTypes: true }).filter(entry => entry.isDirectory() && entry.name === 'muniment.app').map(entry => join(entry.parentPath ?? entry.path, entry.name))
        : [join(target, "muniment.app")];
      if (apps.length !== 1) throw new Error(`Expected one app in ${name}`);
      verifyMacosApp(apps[0], run);
      rmSync(target, { recursive: true, force: true });
    }
  }
};

export const sha256Sums = (entries) => entries.map(([name, digest]) => `${digest}  ${name}\n`).join("");

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

export async function promoteRelease({ token, repository, sha, version, fetchImpl = fetch, verify = verifyNightlyArtifacts }) {
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
  const assets = expectedNightlyAssets(nightly.assets, sha);
  const workDir = mkdtempSync(join(tmpdir(), "muniment-promotion-"));
  let created;
  try {
    // Download and verify every asset before anything is created.
    const files = new Map();
    const digests = new Map();
    for (const asset of assets) {
      const source = await request(fetchImpl, token, asset.url, { headers: { Accept: "application/octet-stream" } });
      const bytes = Buffer.from(await source.arrayBuffer());
      const digest = sha256(bytes);
      if (asset.digest && asset.digest !== `sha256:${digest}`) throw new Error(`downloaded ${asset.name} does not match its release digest`);
      const file = join(workDir, asset.name);
      writeFileSync(file, bytes);
      files.set(asset.name, file);
      digests.set(asset.name, digest);
    }
    await verify({ files, workDir });
    created = await (await request(fetchImpl, token, `${repoApi}/releases`, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ tag_name: version, target_commitish: sha, name: version, body: releaseBody(sha, version), draft: true, prerelease: false }) })).json();
    const upload = (name, contentType, body) => request(fetchImpl, token, `https://uploads.github.com/repos/${repository}/releases/${created.id}/assets?name=${encodeURIComponent(name)}`, { method: "POST", headers: { "Content-Type": contentType }, body });
    const signatures = new Map();
    const stableAssets = [];
    const sums = [];
    for (const asset of assets) {
      const bytes = readFileSync(files.get(asset.name));
      const name = stableAssetName(asset.name, sha, version);
      stableAssets.push({ ...asset, name });
      sums.push([name, digests.get(asset.name)]);
      if (name.endsWith(".sig")) signatures.set(name, bytes.toString("utf8"));
      await upload(name, asset.content_type || "application/octet-stream", bytes);
    }
    const feed = JSON.stringify(createUpdateFeed({ repository, version, assets: stableAssets, signatures }));
    await upload("latest.json", "application/json", feed);
    sums.push(["latest.json", sha256(feed)]);
    await upload("SHA256SUMS", "text/plain", sha256Sums(sums.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))));
    await request(fetchImpl, token, `${repoApi}/releases/${created.id}`, { method: "PATCH", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ draft: false, prerelease: false }) });
  } catch (error) {
    if (created) { await request(fetchImpl, token, `${repoApi}/releases/${created.id}`, { method: "DELETE" }).catch(() => {}); await request(fetchImpl, token, `${repoApi}/git/refs/tags/${encodeURIComponent(version)}`, { method: "DELETE" }).catch(() => {}); }
    throw error;
  } finally {
    rmSync(workDir, { recursive: true, force: true });
  }
}

if (import.meta.url === `file://${process.argv[1]}`) { const [repository, sha, version] = process.argv.slice(2); const token = process.env.GH_TOKEN; await promoteRelease({ token, repository, sha, version }); console.log(`promoted ${sha} to ${version}`); }
