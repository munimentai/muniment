import { execFileSync } from "node:child_process";
import { createHash, generateKeyPairSync } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import {
  APPLE_TEAM_ID, assertGreenCi, assertInstalledNightly, normalizeE2eWaiver, expectedNightlyAssets, forgejoCheckRuns, macosSigningProvenance, microsoftRootPem, promoteRelease, releaseBody,
  sha256Sums, sourceVersion, stableAssetName, verifiedStableCopies, validatePromotionInputs, verifyAuthenticode, verifyMacosApp, verifyMacosPackage, verifyMacosDmg,
  verifyNightlyArtifacts, WINDOWS_PUBLISHER, windowsSigningProvenance,
} from "./release-promotion.mjs";
import { signUpdaterBytes } from "./updater-signature.mjs";

const sha = "a".repeat(40);
const version = "v0.0.1";
const assetNames = [
  `nightly-${sha}-linux-muniment.deb`,
  `nightly-${sha}-linux-muniment_0.0.1_amd64.AppImage`,
  `nightly-${sha}-windows-muniment_0.0.1_x64_en-US.msi`,
  `nightly-${sha}-windows-muniment_0.0.1_x64_en-US-machine.msi`,
  `nightly-${sha}-windows-muniment_0.0.1_x64-nsis.exe`,
  ...['', '-arm64', '-x64'].flatMap(arch => ['.app.zip', '.app.tar.gz', '.app.tar.gz.sig', '.pkg', '.dmg'].map(format => `nightly-${sha}-macos-muniment${arch}${format}`)),
  `nightly-${sha}-linux-muniment_0.0.1_amd64.AppImage.sig`,
  `nightly-${sha}-windows-muniment_0.0.1_x64_en-US.msi.sig`,
  `nightly-${sha}-windows-muniment_0.0.1_x64_en-US-machine.msi.sig`,
  `nightly-${sha}-windows-muniment_0.0.1_x64-nsis.exe.sig`,
  ...["linux", "windows", "macos"].map(platform => `nightly-${sha}-${platform}-muniment-chromium-locales.tar.gz`),
];
const sourceContentType = (name) => name.endsWith(".dmg") ? "application/x-apple-diskimage" : name.endsWith(".deb") ? "application/vnd.debian.binary-package" : name.endsWith(".msi") ? "application/x-msi" : "application/octet-stream";
const assets = assetNames.map((name, id) => ({ id, name, url: `https://api.github.test/assets/${id}`, content_type: sourceContentType(name) }));
// Every non-signature source carries its own bytes so a copy cannot pass as another asset.
const payloadOf = (name) => name.endsWith(".sig") ? Buffer.from("signature fixture").toString("base64") : `payload of ${name}`;
const copyPairs = [
  ["muniment-0.0.1-macos.dmg", "muniment-macos.dmg"],
  ["muniment-0.0.1-macos-arm64.dmg", "muniment-macos-arm64.dmg"],
  ["muniment-0.0.1-macos-x64.dmg", "muniment-macos-x64.dmg"],
  ["muniment-0.0.1-macos.pkg", "muniment-macos.pkg"],
  ["muniment-0.0.1-windows_x64_en-US.msi", "muniment-windows_x64_en-US.msi"],
  ["muniment-0.0.1-windows_x64_en-US-machine.msi", "muniment-windows_x64_en-US-machine.msi"],
  ["muniment-0.0.1-windows_x64-nsis.exe", "muniment-windows_x64-nsis.exe"],
  ["muniment-0.0.1-linux_amd64.AppImage", "muniment-linux_amd64.AppImage"],
  ["muniment-0.0.1-linux.deb", "muniment-linux.deb"],
];
const uploadName = (call) => new URL(call.url).searchParams.get("name");
const smoke = { name: "smoke", status: "completed", conclusion: "success" };
const forgejo = "http://forgejo.test/api/v1/repos/example/muniment";
const installedJobs = ["build (linux)", "build (windows)", "build (macos)", "publish", "linux-e2e", "windows-e2e", "macos-e2e"];
const task = (name, extra = {}) => ({ name, workflow_id: "nightly.yml", head_sha: sha, run_number: 99, status: "success", ...extra });

const response = (body, status = 200, headers = {}) => new Response(
  typeof body === "string" ? body : JSON.stringify(body),
  { status, headers: { "Content-Type": "application/json", ...headers } },
);

const promotionFetch = (overrides = {}) => {
  const calls = [];
  let tagged = false;
  const fetchImpl = async (url, options = {}) => {
    calls.push({ url, options });
    const method = options.method ?? "GET";
    if (overrides.route) {
      const result = await overrides.route(url, method, options, calls);
      if (result) return result;
    }
    if (url === `${forgejo}/tags` && method === "POST") { tagged = !overrides.unmirrored; return response({}, 201); }
    if (url === `${forgejo}/tags/${version}` && method === "DELETE") return response("", 204);
    if (url.includes(`/git/ref/tags/${version}`) && tagged) return response({ object: { type: "commit", sha } });
    if (url.includes(`/git/ref/tags/${version}`) || url.includes(`/releases/tags/${version}`) || url === `${forgejo}/tags/${version}`) return response("missing", 404);
    if (url.startsWith(`${forgejo}/contents/package.json`)) return response({ content: Buffer.from(JSON.stringify({ version: version.slice(1) })).toString("base64") });
    if (url.startsWith(`${forgejo}/contents/core/Cargo.toml`)) return response({ content: Buffer.from(`[workspace]\nmembers = []\n\n[workspace.package]\nversion = "${version.slice(1)}"\n`).toString("base64") });
    if (url.startsWith(`${forgejo}/actions/runs?`)) return response({ workflow_runs: [{ workflow_id: "nightly.yml", commit_sha: sha, status: "success", index_in_repo: 99 }] });
    if (url.startsWith(`${forgejo}/actions/tasks?`)) return response({ workflow_runs: installedJobs.map((name) => task(name)) });
    if (url.startsWith(`${forgejo}/commits/${sha}/status?`)) return response({ statuses: [{ context: "CI / smoke (push)", status: "success" }] });
    if (url.endsWith("/releases/tags/nightly")) return response({
      id: 1, draft: false, prerelease: true, target_commitish: overrides.targetCommitish ?? "stale-branch-value",
      body: `Automated desktop build from ${sha}.\n\n${windowsSigningProvenance(sha)}${overrides.macosSigned !== false ? `\n\n${macosSigningProvenance(sha)}` : "\n\nmacOS artifacts are unsigned pending Apple enrollment Y5DUNHQA74."}`, assets,
    });
    if (url.endsWith("/releases") && method === "POST") return response({ id: 42 });
    if (url.startsWith("https://api.github.test/assets/")) return response(payloadOf(assets[Number(url.split("/").at(-1))].name));
    if (url.startsWith("https://uploads.github.com/")) return response({});
    if (url.endsWith("/releases/42") && ["PATCH", "DELETE"].includes(method)) return response({});
    if (url.includes(`/git/refs/tags/${version}`) && method === "DELETE") return response({});
    throw new Error(`unexpected request: ${method} ${url}`);
  };
  return { calls, fetchImpl };
};

// The promotion flow tests inject the artifact verifier; the verifier's own
// checks are covered below against tool output fixtures.
const promote = (fetchImpl, verify = async () => {}, options = {}) => promoteRelease({
  token: "token", repository: "owner/repo", sha, version, repoHost: { api: forgejo, token: "repo-token" },
  fetchImpl, verify, wait: async () => {}, tagAttempts: 3, log: () => {}, ...options,
});

describe("stable release promotion", () => {
  it("uses fresh connections across verification and release writes", async () => {
    const fixture = promotionFetch();
    const sockets = new Set();
    const server = createServer(async (req, res) => {
      sockets.add(req.socket);
      const chunks = [];
      for await (const chunk of req) chunks.push(chunk);
      const result = await fixture.fetchImpl(new URL(req.url, "http://localhost").searchParams.get("url"), {
        method: req.method, headers: req.headers, body: Buffer.concat(chunks),
      });
      res.writeHead(result.status);
      res.end(Buffer.from(await result.arrayBuffer()));
    });
    await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
    try {
      await promote((url, options) => fetch(`http://127.0.0.1:${server.address().port}/?url=${encodeURIComponent(url)}`, options));
      expect(fixture.calls.some(({ options }) => options.method === "POST")).toBe(true);
      expect(sockets.size).toBe(fixture.calls.length);
    } finally {
      server.closeAllConnections();
      await new Promise(resolve => server.close(resolve));
    }
  });

  it("accepts only exact lowercase SHAs and strict stable SemVer tags", () => {
    expect(() => validatePromotionInputs(sha, "v1.2.3")).not.toThrow();
    for (const invalid of ["1.2.3", "v1.2", "v01.2.3", "v1.2.3-beta", "v1.2.3 "]) expect(() => validatePromotionInputs(sha, invalid)).toThrow("strict SemVer");
    for (const invalid of ["a".repeat(39), "A".repeat(40), `${sha}0`, "g".repeat(40)]) expect(() => validatePromotionInputs(invalid, "v1.2.3")).toThrow("40 lowercase");
  });

  it("requires exactly one of each finalized nightly artifact", () => {
    expect(expectedNightlyAssets(assets, sha)).toEqual(assets);
    expect(() => expectedNightlyAssets(assets.slice(1), sha)).toThrow("exactly 27");
    expect(() => expectedNightlyAssets([...assets.slice(0, 12), assets[0]], sha)).toThrow();
  });

  it("requires smoke and rejects pending or otherwise-named failed checks", () => {
    expect(() => assertGreenCi([smoke], sha)).not.toThrow();
    expect(() => assertGreenCi([], sha)).toThrow("CI is not green");
    expect(() => assertGreenCi([{ ...smoke, status: "in_progress", conclusion: null }], sha)).toThrow("CI is not green");
    expect(() => assertGreenCi([smoke, { name: "promote", status: "in_progress", conclusion: null }], sha)).not.toThrow();
    expect(() => assertGreenCi([smoke, { name: "security", status: "completed", conclusion: "failure" }], sha)).toThrow("CI is not green");
  });

  it("reads Forgejo job statuses as checks without the promotion's own jobs", () => {
    const checks = forgejoCheckRuns([
      { context: "CI / smoke (push)", status: "success" },
      { context: "secret-scan / gitleaks (push)", status: "pending" },
      { context: "Promote stable desktop release / promote (workflow_dispatch)", status: "pending" },
      { context: "Promote stable desktop release / Draft WinGet manifest PR (workflow_dispatch)", status: "pending" },
    ]);
    expect(checks).toEqual([smoke, { name: "gitleaks", status: "in_progress", conclusion: "pending" }]);
    expect(() => assertGreenCi(checks, sha)).toThrow("CI is not green");
    expect(() => assertGreenCi(checks.slice(0, 1), sha)).not.toThrow();
  });

  it("publishes signing provenance for both platforms", () => {
    const body = releaseBody(sha, version);
    expect(body).toContain(sha);
    expect(body).toContain("Windows installers are signed");
    expect(body).toContain(macosSigningProvenance(sha));
    expect(body).not.toContain("unsigned");
    expect(body).toContain("Model weights are not included");
  });

  it.each([["tag", `/git/ref/tags/${version}`], ["release", `/releases/tags/${version}`], ["tag", `${forgejo}/tags/${version}`]])("rejects an existing %s at %s", async (kind, endpoint) => {
    const { fetchImpl } = promotionFetch({ route: (url) => url.includes(endpoint) ? response({}) : null });
    await expect(promote(fetchImpl)).rejects.toThrow(`${kind} ${version} already exists`);
  });

  it("rejects a source-version mismatch before inspecting nightly", async () => {
    const { calls, fetchImpl } = promotionFetch({ route: (url) => url.includes("/contents/package.json") ? response({ content: Buffer.from('{"version":"9.9.9"}').toString("base64") }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("does not match");
    expect(calls.some(({ url }) => url.endsWith("/releases/tags/nightly"))).toBe(false);
  });

  it("rejects a core version that differs from the release", async () => {
    const { calls, fetchImpl } = promotionFetch({ route: (url) => url.includes("/contents/core/Cargo.toml") ? response({ content: Buffer.from('[workspace.package]\nversion = "9.9.9"\n').toString("base64") }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow(`core/Cargo.toml version 9.9.9 does not match ${version}`);
    expect(calls.some(({ url }) => url.endsWith("/releases/tags/nightly"))).toBe(false);
  });

  it("reads the workspace version of a manifest", () => {
    expect(sourceVersion('[workspace]\nmembers = []\n\n# One version.\n[workspace.package]\nversion = "0.4.0"\n')).toBe("0.4.0");
    expect(sourceVersion('[package]\nversion = "0.4.0"\n')).toBe(null);
  });

  it("rejects a later-page CI failure", async () => {
    const successes = Array.from({ length: 50 }, (_, index) => ({ context: index === 0 ? "CI / smoke (push)" : `CI / check-${index} (push)`, status: "success" }));
    const { fetchImpl } = promotionFetch({ route: (url) => url.includes(`/commits/${sha}/status?`) ? response({ statuses: new URL(url).searchParams.get("page") === "1" ? successes : [{ context: "CI / late failure (push)", status: "failure" }] }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("CI is not green");
  });

  it("uses the nightly tag despite stale target_commitish and copies exactly 27 assets without mutating nightly", async () => {
    const { calls, fetchImpl } = promotionFetch();
    await promote(fetchImpl);
    const tag = calls.findIndex(({ url, options }) => url === `${forgejo}/tags` && options.method === "POST");
    expect(JSON.parse(calls[tag].options.body)).toEqual({ tag_name: version, target: sha });
    const createIndex = calls.findIndex(({ url, options }) => url.endsWith("/releases") && options.method === "POST");
    expect(tag).toBeGreaterThan(-1);
    expect(createIndex).toBeGreaterThan(tag);
    const create = calls[createIndex];
    expect(create.url).toBe("https://api.github.com/repos/owner/repo/releases");
    expect(JSON.parse(create.options.body)).toMatchObject({ tag_name: version, target_commitish: sha, draft: true, prerelease: false });
    expect(calls.filter(({ url }) => url.startsWith("https://api.github.test/assets/"))).toHaveLength(27);
    expect(calls.filter(({ url }) => url.startsWith("https://uploads.github.com/"))).toHaveLength(38);
    expect(calls.some(({ url }) => url.endsWith("/git/ref/tags/nightly"))).toBe(false);
    expect(calls.some(({ url, options }) => url.includes("/releases/1") && options.method)).toBe(false);
    const uploads = calls.filter(({ url }) => url.startsWith("https://uploads.github.com/"));
    const names = uploads.map(({ url }) => new URL(url).searchParams.get("name"));
    expect(names).not.toEqual(expect.arrayContaining([expect.stringContaining("nightly-")]));
    expect(names).toContain("muniment-0.0.1-macos.pkg");
    const feed = JSON.parse(uploads.find(({ url }) => url.endsWith("name=latest.json")).options.body);
    for (const platform of Object.values(feed.platforms)) {
      expect(names).toContain(decodeURIComponent(platform.url.split("/").at(-1)));
      expect(platform.signature).toBe(Buffer.from("signature fixture").toString("base64"));
    }
    const sums = Buffer.from(uploads.find(({ url }) => url.endsWith("name=SHA256SUMS")).options.body).toString();
    const digest = (text) => createHash("sha256").update(text).digest("hex");
    const feedBody = uploads.find(({ url }) => url.endsWith("name=latest.json")).options.body;
    expect(sums.trim().split("\n")).toHaveLength(28);
    expect(sums).toContain(`${digest("payload of nightly-" + sha + "-macos-muniment.pkg")}  muniment-0.0.1-macos.pkg\n`);
    expect(sums).not.toMatch(/ muniment-(?!0\.0\.1-)/);
    expect(sums).toContain(`${digest(feedBody)}  latest.json\n`);
    const copyNames = copyPairs.map(([, alias]) => alias);
    for (const upload of uploads.filter((candidate) => !["latest.json", "SHA256SUMS", ...copyNames].includes(uploadName(candidate)))) {
      const nightlyName = assetNames.find((candidate) => stableAssetName(candidate, sha, version) === uploadName(upload));
      expect(Buffer.from(upload.options.body).toString()).toBe(payloadOf(nightlyName));
    }
    const publish = calls.find(({ url, options }) => url.endsWith("/releases/42") && options.method === "PATCH");
    expect(JSON.parse(publish.options.body)).toEqual({ draft: false, prerelease: false });
  });

  it("uploads nine version-less copies after metadata and before the draft is published", async () => {
    const { calls, fetchImpl } = promotionFetch();
    await promote(fetchImpl);
    const uploads = calls.filter(({ url }) => url.startsWith("https://uploads.github.com/"));
    expect(uploads).toHaveLength(38);
    const names = uploads.map(uploadName);
    expect(new Set(names).size).toBe(38);
    const copyNames = copyPairs.map(([, alias]) => alias);
    expect(names.slice(-9)).toEqual(copyNames);
    expect(names.slice(0, 29).filter((name) => !name.startsWith("muniment-0.0.1-"))).toEqual(["latest.json", "SHA256SUMS"]);
    expect(names.indexOf("SHA256SUMS")).toBe(28);
    const publishIndex = calls.findIndex(({ url, options }) => url.endsWith("/releases/42") && options.method === "PATCH");
    expect(publishIndex).toBeGreaterThan(calls.indexOf(uploads.at(-1)));
    for (const [source, alias] of copyPairs) {
      const versioned = uploads.find((upload) => uploadName(upload) === source);
      const copy = uploads.find((upload) => uploadName(upload) === alias);
      expect(Buffer.from(copy.options.body).equals(Buffer.from(versioned.options.body))).toBe(true);
      expect(copy.options.headers["Content-Type"]).toBe(versioned.options.headers["Content-Type"]);
    }
    const copyType = (alias) => uploads.find((upload) => uploadName(upload) === alias).options.headers["Content-Type"];
    expect(copyType("muniment-macos.dmg")).toBe("application/x-apple-diskimage");
    expect(copyType("muniment-linux.deb")).toBe("application/vnd.debian.binary-package");
    expect(copyType("muniment-windows_x64_en-US.msi")).toBe("application/x-msi");
    const sums = Buffer.from(uploads.find((upload) => uploadName(upload) === "SHA256SUMS").options.body).toString();
    for (const [source, alias] of copyPairs) {
      const copyBytes = uploads.find((upload) => uploadName(upload) === alias).options.body;
      expect(sums).toContain(`${createHash("sha256").update(copyBytes).digest("hex")}  ${source}\n`);
      expect(sums).not.toContain(alias);
    }
    const feed = JSON.parse(uploads.find((upload) => uploadName(upload) === "latest.json").options.body);
    for (const platform of Object.values(feed.platforms)) expect(platform.url).toContain("/releases/download/v0.0.1/muniment-0.0.1-");
    expect(JSON.stringify(feed)).not.toMatch(/download\/v0\.0\.1\/muniment-(?!0\.0\.1-)/);
  });

  it("does not alias native-architecture packages, app archives, signatures or language packs", async () => {
    const { calls, fetchImpl } = promotionFetch();
    await promote(fetchImpl);
    const aliases = calls.filter(({ url }) => url.startsWith("https://uploads.github.com/")).map(uploadName).filter((name) => !name.startsWith("muniment-0.0.1-") && name !== "latest.json" && name !== "SHA256SUMS");
    expect(aliases).toHaveLength(9);
    for (const name of aliases) expect(name).not.toMatch(/\.sig$|\.app\.|chromium-locales|-arm64\.pkg|-x64\.pkg/);
  });

  it("verifies every downloaded asset before creating a release, whatever the release text claims", async () => {
    const { calls, fetchImpl } = promotionFetch({ macosSigned: false });
    const verify = vi.fn(async ({ files }) => {
      expect([...files.keys()]).toEqual(assetNames);
      for (const file of files.values()) expect(readFileSync(file).length).toBeGreaterThan(0);
      expect(calls.some(({ options }) => options.method === "POST")).toBe(false);
    });
    await promote(fetchImpl, verify);
    expect(verify).toHaveBeenCalledTimes(1);
  });

  it("rejects an asset that fails verification before creating a release", async () => {
    const { calls, fetchImpl } = promotionFetch();
    await expect(promote(fetchImpl, async () => { throw new Error("muniment.pkg is not signed"); })).rejects.toThrow("not signed");
    expect(calls.some(({ options }) => options.method === "POST")).toBe(false);
  });

  it("rejects downloaded bytes that differ from the release digest", async () => {
    const { calls, fetchImpl } = promotionFetch({ route: (url) => url.endsWith("/releases/tags/nightly") ? response({
      draft: false, prerelease: true, body: `Built from ${sha}`, assets: assets.map((asset) => ({ ...asset, digest: `sha256:${"0".repeat(64)}` })),
    }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("does not match its release digest");
    expect(calls.some(({ options }) => options.method === "POST")).toBe(false);
  });

  it.each(["linux-e2e", "windows-e2e", "macos-e2e", "publish"])("rejects a nightly missing %s", async (missing) => {
    const { calls, fetchImpl } = promotionFetch({ route: (url) => url.includes("/actions/tasks?") ? response({ workflow_runs: installedJobs.filter((name) => name !== missing).map((name) => task(name)) }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("No successful full installed nightly");
    expect(calls.some(({ options }) => options.method === "POST")).toBe(false);
  });

  it.each(["failure", "skipped"])("rejects an installed check with conclusion %s", async (conclusion) => {
    const { fetchImpl } = promotionFetch({ route: (url) => url.includes("/actions/tasks?") ? response({ workflow_runs: installedJobs.map((name) => task(name, { status: name === "macos-e2e" ? conclusion : "success" })) }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("No successful full installed nightly");
  });

  it("judges each job by its newest attempt", async () => {
    const retried = (status) => promotionFetch({ route: (url) => url.includes("/actions/tasks?") ? response({ workflow_runs: [task("macos-e2e", { status }), ...installedJobs.map((name) => task(name, { status: name === "macos-e2e" ? (status === "success" ? "failure" : "success") : "success" }))] }) : null });
    await expect(promote(retried("success").fetchImpl)).resolves.toBeUndefined();
    await expect(promote(retried("failure").fetchImpl)).rejects.toThrow("No successful full installed nightly");
  });

  it("rejects a successful nightly for another revision", async () => {
    const { fetchImpl } = promotionFetch({ route: (url) => url.includes("/actions/runs?") ? response({ workflow_runs: [{ workflow_id: "nightly.yml", commit_sha: "b".repeat(40), status: "success", index_in_repo: 99 }] }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("No successful full installed nightly");
  });

  it("stops a dry run after verification without a tag or release", async () => {
    const { calls, fetchImpl } = promotionFetch();
    const verify = vi.fn(async () => {});
    await promote(fetchImpl, verify, { dryRun: true });
    expect(verify).toHaveBeenCalledTimes(1);
    expect(calls.filter(({ options }) => ["POST", "PATCH", "DELETE"].includes(options.method))).toEqual([]);
  });

  it("removes the Forgejo tag and creates no release when the mirror does not carry it", async () => {
    const { calls, fetchImpl } = promotionFetch({ unmirrored: true });
    await expect(promote(fetchImpl)).rejects.toThrow("did not carry tag");
    expect(calls.some(({ url, options }) => url.endsWith("/releases") && options.method === "POST")).toBe(false);
    expect(calls.some(({ url, options }) => url === `${forgejo}/tags/${version}` && options.method === "DELETE")).toBe(true);
  });

  it("requires the Forgejo repository API and token", async () => {
    const { fetchImpl } = promotionFetch();
    await expect(promote(fetchImpl, undefined, { repoHost: { api: forgejo } })).rejects.toThrow("Forgejo repository API");
    await expect(promote(fetchImpl, undefined, { repoHost: { api: "https://api.github.com/repos/o/r", token: "t" } })).rejects.toThrow("Forgejo repository API");
  });

  it("fails closed when the nightly release is not finalized at the source commit", async () => {
    const { calls, fetchImpl } = promotionFetch({ route: (url) => url.endsWith("/releases/tags/nightly") ? response({ draft: false, prerelease: true, body: "Built from another commit", assets }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("not finalized");
    expect(calls.some(({ url, options }) => url.endsWith("/releases") && options.method === "POST")).toBe(false);
  });

  it("cleans up the draft release and tag after a partial upload failure", async () => {
    let uploads = 0;
    const { calls, fetchImpl } = promotionFetch({ route: (url) => {
      if (url.startsWith("https://uploads.github.com/") && ++uploads === 2) return response("upload failed", 500);
      return null;
    } });
    await expect(promote(fetchImpl)).rejects.toThrow("500");
    expect(calls.some(({ url, options }) => url.endsWith("/releases/42") && options.method === "DELETE")).toBe(true);
    expect(calls.some(({ url, options }) => url === `${forgejo}/tags/${version}` && options.method === "DELETE")).toBe(true);
    expect(calls.some(({ url, options }) => url.includes(`/git/refs/tags/${version}`) && options.method === "DELETE")).toBe(true);
  });

  it.each(copyPairs.map(([source]) => source))("rejects a changed source %s before any upload or release", async (source) => {
    const { calls, fetchImpl } = promotionFetch();
    const nightlyName = assetNames.find((candidate) => stableAssetName(candidate, sha, version) === source);
    const tamper = async ({ files }) => writeFileSync(files.get(nightlyName), "tampered bytes");
    await expect(promote(fetchImpl, tamper)).rejects.toThrow(`${source} does not match its SHA256SUMS entry`);
    expect(calls.some(({ url }) => url.startsWith("https://uploads.github.com/"))).toBe(false);
    expect(calls.some(({ options }) => options.method === "POST")).toBe(false);
  });

  it("rejects a missing copy source or checksum entry for the whole batch", () => {
    const directory = mkdtempSync(path.join(tmpdir(), "muniment-copies-"));
    try {
      const sources = new Map(copyPairs.map(([source]) => {
        const file = path.join(directory, source);
        writeFileSync(file, `payload of ${source}`);
        return [source, { file, contentType: "application/octet-stream" }];
      }));
      const sums = sha256Sums(copyPairs.map(([source]) => [source, createHash("sha256").update(`payload of ${source}`).digest("hex")]));
      expect(verifiedStableCopies({ version, sources, sums }).map(({ name }) => name)).toEqual(copyPairs.map(([, alias]) => alias));
      const withoutSource = new Map(sources);
      withoutSource.delete("muniment-0.0.1-linux.deb");
      expect(() => verifiedStableCopies({ version, sources: withoutSource, sums })).toThrow("muniment-linux.deb has no versioned source");
      expect(() => verifiedStableCopies({ version, sources, sums: sums.split("\n").filter((line) => !line.endsWith("-linux.deb")).join("\n") })).toThrow("muniment-0.0.1-linux.deb has no SHA256SUMS entry");
      expect(() => verifiedStableCopies({ version, sources, sums: sums.replace(/^./, "0") })).toThrow("does not match its SHA256SUMS entry");
    } finally { rmSync(directory, { recursive: true, force: true }); }
  });

  it("uploads no copy when the uploaded checksums no longer match a late candidate", async () => {
    let files;
    const lateName = `nightly-${sha}-linux-muniment.deb`;
    const { calls, fetchImpl } = promotionFetch({ route: (url, method) => {
      if (url.startsWith("https://uploads.github.com/") && new URL(url).searchParams.get("name") === "SHA256SUMS") writeFileSync(files.get(lateName), "changed after checksums");
      return null;
    } });
    await expect(promote(fetchImpl, async (input) => { files = input.files; })).rejects.toThrow("muniment-0.0.1-linux.deb does not match its SHA256SUMS entry");
    const names = calls.filter(({ url }) => url.startsWith("https://uploads.github.com/")).map(uploadName);
    expect(names).toHaveLength(29);
    expect(names.some((name) => copyPairs.some(([, alias]) => alias === name))).toBe(false);
    expect(calls.some(({ url, options }) => url.endsWith("/releases/42") && options.method === "PATCH")).toBe(false);
    expect(calls.some(({ url, options }) => url.endsWith("/releases/42") && options.method === "DELETE")).toBe(true);
    expect(calls.some(({ url, options }) => url === `${forgejo}/tags/${version}` && options.method === "DELETE")).toBe(true);
  });

  it("cleans up the draft release and tag when a copy upload fails", async () => {
    const { calls, fetchImpl } = promotionFetch({ route: (url) => url.startsWith("https://uploads.github.com/") && new URL(url).searchParams.get("name") === "muniment-macos.pkg" ? response("copy failed", 500) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("500");
    expect(calls.some(({ url, options }) => url.endsWith("/releases/42") && options.method === "PATCH")).toBe(false);
    expect(calls.some(({ url, options }) => url.endsWith("/releases/42") && options.method === "DELETE")).toBe(true);
    expect(calls.some(({ url, options }) => url === `${forgejo}/tags/${version}` && options.method === "DELETE")).toBe(true);
    expect(calls.some(({ url, options }) => url.includes(`/git/refs/tags/${version}`) && options.method === "DELETE")).toBe(true);
  });

  it("reports all nine copies in a dry run without any remote mutation", async () => {
    const { calls, fetchImpl } = promotionFetch();
    const lines = [];
    await promote(fetchImpl, undefined, { dryRun: true, log: (line) => lines.push(line) });
    for (const [source, alias] of copyPairs) expect(lines).toContain(`dry run: ${source} -> ${alias}`);
    expect(calls.filter(({ options }) => ["POST", "PATCH", "DELETE"].includes(options.method))).toEqual([]);
  });

  it("fails a dry run when a copy source changed", async () => {
    const { calls, fetchImpl } = promotionFetch();
    const lateName = `nightly-${sha}-linux-muniment.deb`;
    await expect(promote(fetchImpl, async ({ files }) => writeFileSync(files.get(lateName), "changed"), { dryRun: true })).rejects.toThrow("does not match its SHA256SUMS entry");
    expect(calls.filter(({ options }) => ["POST", "PATCH", "DELETE"].includes(options.method))).toEqual([]);
  });
});


it.each([
  ["release-promotion.mjs", ["owner/repo", sha, version]],
  ["winget-release.mjs", ["owner/repo", version, "/tmp/unused-winget-fixture"]],
])("%s reads API authorization from the environment", (script, args) => {
  const directory = mkdtempSync(path.join(tmpdir(), "muniment-release-auth-"));
  try {
    const preload = path.join(directory, "fetch.mjs");
    writeFileSync(preload, `
      globalThis.fetch = async (_url, options) => {
        if (options.headers.Authorization !== 'Bearer ' + process.env.GH_TOKEN) throw new Error('wrong header');
        if (process.argv.some(arg => arg.includes(process.env.GH_TOKEN))) throw new Error('token in argv');
        throw new Error('AUTH_CHECK_PASSED');
      };
    `);
    expect(() => execFileSync(process.execPath, ["--import", preload,
      path.resolve(".github/lib", script), ...args], {
      env: { ...process.env, GH_TOKEN: "fixture-only-release-token", REPO_TOKEN: "fixture-only-repo-token", REPO_API: forgejo }, stdio: "pipe", timeout: 10000,
    })).toThrow(/AUTH_CHECK_PASSED/);
  } finally { rmSync(directory, { recursive: true, force: true }); }
});

it("names versioned installers and their signatures consistently", () => {
  const name = `nightly-${sha}-windows-muniment_0.0.1_x64_en-US-machine.msi`;
  expect(stableAssetName(name, sha, version)).toBe("muniment-0.0.1-windows_x64_en-US-machine.msi");
  expect(stableAssetName(`${name}.sig`, sha, version)).toBe(`${stableAssetName(name, sha, version)}.sig`);
  expect(() => stableAssetName(name, "b".repeat(40), version)).toThrow("Unexpected");
});

describe("nightly artifact verification", () => {
  const withDirectory = (test) => {
    const directory = mkdtempSync(path.join(tmpdir(), "muniment-verify-"));
    try { return test(directory); } finally { rmSync(directory, { recursive: true, force: true }); }
  };
  const result = (stdout, status = 0) => ({ status, stdout, stderr: "" });
  const authenticode = (subject) => `Signature Index: 0  (Primary Signature)\n\nSigner's certificate:\n\t------------------\n\tSigner #0:\n\t\tSubject: ${subject}\n\t\tIssuer : CN=Microsoft ID Verified CS EOC CA 04\n\nSignature verification: ok\n\nNumber of verified signatures: 1\nSucceeded\n`;
  const appleInfo = (kind, team = APPLE_TEAM_ID, extra = "") => `- path: file\n  entity:\n    cms:\n      certificates:\n      - subject: 'CN=Developer ID ${kind}: Green Kangaroo, LLC (${team}), OU=${team}'\n        chains_to_apple_root_ca: true\n      signers:\n      - issuer: CN=Developer ID Certification Authority\n        signature_verifies: true\n${extra}`;

  it("pins the Microsoft root that Azure Artifact Signing chains to", () => {
    expect(microsoftRootPem()).toContain("-----BEGIN CERTIFICATE-----");
    expect(() => microsoftRootPem(Buffer.from("other"))).toThrow("pinned SHA-256");
  });

  it("accepts Authenticode only from the publisher", () => {
    const run = vi.fn(() => result(authenticode(WINDOWS_PUBLISHER)));
    verifyAuthenticode("/tmp/muniment.msi", "/tmp/ca.pem", run);
    expect(run.mock.calls[0].slice(0, 2)).toEqual(["osslsigncode", ["verify", "-CAfile", "/tmp/ca.pem", "-TSA-CAfile", "/tmp/ca.pem", "-in", "/tmp/muniment.msi"]]);
    expect(() => verifyAuthenticode("/tmp/muniment.msi", "/tmp/ca.pem", () => result(authenticode("CN=Someone Else")))).toThrow("signed by CN=Someone Else");
    expect(() => verifyAuthenticode("/tmp/muniment.msi", "/tmp/ca.pem", () => result("Failed\n", 1))).toThrow("Authenticode verification failed");
  });

  it("accepts OpenSSL's exact publisher rendering and rejects changed identity fields", () => {
    const subject = "/C=US/ST=South Carolina/L=Murrells Inlet/O=Green Kangaroo, LLC/CN=Green Kangaroo, LLC";
    verifyAuthenticode("/tmp/muniment.exe", "/tmp/ca.pem", () => result(authenticode(subject)));
    for (const changed of [subject.replace("/C=US", "/C=CA"), subject.replace("/O=Green Kangaroo, LLC", "/O=Someone Else"), `${subject}/OU=Other`]) {
      expect(() => verifyAuthenticode("/tmp/muniment.exe", "/tmp/ca.pem", () => result(authenticode(changed)))).toThrow("signed by");
    }
  });

  it("checks every Mach-O file and the stapled ticket in an app", () => withDirectory((directory) => {
    const app = path.join(directory, "muniment.app");
    mkdirSync(path.join(app, "Contents", "MacOS"), { recursive: true });
    writeFileSync(path.join(app, "Contents", "MacOS", "muniment-desktop"), Buffer.from("cffaedfe00", "hex"));
    writeFileSync(path.join(app, "Contents", "Info.plist"), "<plist/>");
    expect(() => verifyMacosApp(app, () => result(appleInfo("Application")))).toThrow("no stapled notarization ticket");
    writeFileSync(path.join(app, "Contents", "CodeResources"), "ticket");
    const run = vi.fn((command, args) => result(args[0] === "verify" ? "no problems detected!" : appleInfo("Application")));
    verifyMacosApp(app, run);
    expect(run.mock.calls.map(([, args]) => args[0])).toEqual(["verify", "print-signature-info"]);
    expect(() => verifyMacosApp(app, (command, args) => result("", args[0] === "verify" ? 1 : 0))).toThrow("rcodesign rejects");
    expect(() => verifyMacosApp(app, () => result(appleInfo("Application", "Y0THERTEAM")))).toThrow(`team ${APPLE_TEAM_ID}`);
    expect(() => verifyMacosApp(app, () => result(appleInfo("Application").replace("chains_to_apple_root_ca: true", "chains_to_apple_root_ca: false")))).toThrow("Apple root");
    expect(() => verifyMacosApp(app, () => result(appleInfo("Application").replace("  signature_verifies: true", "  signature_verifies: false")))).toThrow("does not verify");
  }));

  it("requires a valid Application identity for disk images", () => {
    const valid = appleInfo("Application");
    verifyMacosDmg("/tmp/muniment.dmg", () => result(valid));
    expect(() => verifyMacosDmg("/tmp/muniment.dmg", () => result("", 1))).toThrow("cannot read");
    expect(() => verifyMacosDmg("/tmp/muniment.dmg", () => result(appleInfo("Installer")))).toThrow("Developer ID Application");
    expect(() => verifyMacosDmg("/tmp/muniment.dmg", () => result(valid.replace("signature_verifies: true", "signature_verifies: false")))).toThrow("does not verify");
  });

  it("extracts disk images and verifies the contained app", () => withDirectory(directory => {
    const file = path.join(directory, "muniment.dmg");
    const run = vi.fn((command, args) => {
      if (command === "7z") {
        if (!args.includes("-x!Muniment/Applications")) return result("Dangerous link path: /Applications", 2);
        const target = args.find(arg => arg.startsWith("-o")).slice(2);
        const app = path.join(target, "Muniment", "muniment.app", "Contents");
        mkdirSync(path.join(app, "MacOS"), { recursive: true });
        writeFileSync(path.join(app, "CodeResources"), "ticket");
        writeFileSync(path.join(app, "MacOS", "muniment"), Buffer.from("cffaedfe00", "hex"));
      }
      return result(appleInfo("Application"));
    });
    verifyNightlyArtifacts({ files: new Map([["muniment.dmg", file]]), workDir: directory, run, caPem: "pem" });
    expect(run.mock.calls.some(([command, args]) => command === "rcodesign" && args[0] === "verify" && args[1].endsWith("Contents/MacOS/muniment"))).toBe(true);
  }));

  it("checks the installer package signature", () => {
    const valid = appleInfo("Installer", APPLE_TEAM_ID, "      checksum_verifies: true\n      rsa_signature_verifies: false\n      cms_signature_verifies: true\n");
    verifyMacosPackage("/tmp/muniment.pkg", () => result(valid));
    expect(() => verifyMacosPackage("/tmp/muniment.pkg", () => result(valid.replace("cms_signature_verifies: true", "cms_signature_verifies: false")))).toThrow("does not verify");
    expect(() => verifyMacosPackage("/tmp/muniment.pkg", () => result(appleInfo("Application", APPLE_TEAM_ID, "      checksum_verifies: true\n      cms_signature_verifies: true\n")))).toThrow("Developer ID Installer");
  });

  it("checks each updater signature against the committed public key", () => withDirectory((directory) => {
    const { privateKey, publicKey } = generateKeyPairSync("ed25519");
    const keyId = Buffer.from("0102030405060708", "hex");
    const pk = publicKey.export({ format: "der", type: "spki" }).subarray(-32);
    const publicText = Buffer.from(`untrusted comment: minisign public key\n${Buffer.concat([Buffer.from("Ed"), keyId, pk]).toString("base64")}\n`).toString("base64");
    const name = `nightly-${sha}-linux-muniment.AppImage`;
    const file = path.join(directory, name);
    writeFileSync(file, "appimage bytes");
    writeFileSync(`${file}.sig`, signUpdaterBytes(Buffer.from("appimage bytes"), { keyId, privateKey }, { fileName: "muniment.AppImage", version: "0.0.1" }));
    const files = new Map([[name, file], [`${name}.sig`, `${file}.sig`]]);
    verifyNightlyArtifacts({ files, workDir: directory, publicKey: publicText, run: vi.fn(), caPem: "pem" });
    writeFileSync(file, "tampered bytes");
    expect(() => verifyNightlyArtifacts({ files, workDir: directory, publicKey: publicText, run: vi.fn(), caPem: "pem" })).toThrow(`${name} updater signature`);
    files.delete(`${name}.sig`);
    expect(() => verifyNightlyArtifacts({ files, workDir: directory, publicKey: publicText, run: vi.fn(), caPem: "pem" })).toThrow("no updater signature");
  }));

  it("writes SHA256SUMS in the sha256sum check format", () => {
    expect(sha256Sums([["a.msi", "0".repeat(64)], ["latest.json", "f".repeat(64)]])).toBe(`${"0".repeat(64)}  a.msi\n${"f".repeat(64)}  latest.json\n`);
  });
});

describe("single E2E waiver", () => {
  const failedRun = (extra = {}) => ({ workflow_id: "nightly.yml", commit_sha: sha, status: "failure", index_in_repo: 99, ...extra });
  const waivedJobs = (waived, status = "failure", overrides = {}) => [...installedJobs, "verify-requested-e2e"].map((name) => task(name, {
    status: overrides[name] ?? (name === waived ? status : name === "verify-requested-e2e" ? "skipped" : "success"),
  }));
  const waiverFetch = ({ runs = [failedRun()], tasks = waivedJobs("macos-e2e"), statuses = [{ context: "CI / smoke (push)", status: "success" }] } = {}) => promotionFetch({ route: (url) => {
    if (url.startsWith(`${forgejo}/actions/runs?`)) return response({ workflow_runs: runs });
    if (url.startsWith(`${forgejo}/actions/tasks?`)) return response({ workflow_runs: tasks });
    if (url.startsWith(`${forgejo}/commits/${sha}/status?`)) return response({ statuses });
    return null;
  } });
  const withSummary = async (test) => {
    const directory = mkdtempSync(path.join(tmpdir(), "muniment-waiver-"));
    const summaryFile = path.join(directory, "summary.md");
    writeFileSync(summaryFile, "");
    try { return await test(summaryFile); } finally { rmSync(directory, { recursive: true, force: true }); }
  };
  const macosWaiver = { job: "macos-e2e", reason: "macOS runner lost its display session" };

  it("treats an absent or blank waiver as none", () => {
    for (const none of [undefined, null, {}, { job: "", reason: "" }, { job: "  ", reason: " \n " }]) expect(normalizeE2eWaiver(none)).toBeNull();
    expect(normalizeE2eWaiver({ job: " windows-e2e ", reason: " flaky VM \n" })).toEqual({ job: "windows-e2e", reason: "flaky VM" });
  });

  it.each([
    [{ job: "build (macos)", reason: "r" }, "must be one of"],
    [{ job: "publish", reason: "r" }, "must be one of"],
    [{ job: "verify-requested-e2e", reason: "r" }, "must be one of"],
    [{ job: "", reason: "r" }, "must be one of"],
    [{ job: "linux-e2e,macos-e2e", reason: "r" }, "only one job"],
    [{ job: "linux-e2e macos-e2e", reason: "r" }, "only one job"],
    [{ job: ["macos-e2e"], reason: "r" }, "only one job"],
    [{ job: "macos-e2e", reason: "" }, "requires a reason"],
    [{ job: "macos-e2e", reason: " \n\t " }, "requires a reason"],
    [{ job: "macos-e2e" }, "requires a reason"],
    ["macos-e2e", "name one job"],
  ])("rejects waiver %j before any request", async (waiver, message) => {
    const { calls, fetchImpl } = waiverFetch();
    const verify = vi.fn(async () => {});
    await expect(promote(fetchImpl, verify, { e2eWaiver: waiver })).rejects.toThrow(message);
    await expect(assertInstalledNightly(fetchImpl, "repo-token", forgejo, sha, waiver)).rejects.toThrow(message);
    expect(calls).toHaveLength(0);
    expect(verify).not.toHaveBeenCalled();
  });

  it("promotes a failed nightly whose only failure is the waived macos-e2e and audits it privately", () => withSummary(async (summaryFile) => {
    const { calls, fetchImpl } = waiverFetch({ statuses: [
      { context: "CI / smoke (push)", status: "success" },
      { context: "Nightly desktop release / macos-e2e (workflow_dispatch)", status: "failure" },
    ] });
    const lines = [];
    const reason = "macOS runner lost its display session.\nInstalled checks passed on a manual rerun.";
    await promote(fetchImpl, undefined, { e2eWaiver: { job: "macos-e2e", reason }, summaryFile, log: (line) => lines.push(line) });
    expect(calls.some(({ url }) => url.includes("/actions/runs?") && url.includes("status=success"))).toBe(false);
    const log = lines.join("\n");
    expect(log).toContain("E2E waiver job: macos-e2e");
    expect(log).toContain("| macOS runner lost its display session.\n| Installed checks passed on a manual rerun.");
    expect(lines.find((line) => line.startsWith("::warning"))).not.toContain("\n");
    const summary = readFileSync(summaryFile, "utf8");
    expect(summary).toContain("`macos-e2e`");
    expect(summary).toContain("> macOS runner lost its display session.\n> Installed checks passed on a manual rerun.");
    const create = calls.find(({ url, options }) => url.endsWith("/releases") && options.method === "POST");
    const body = JSON.parse(create.options.body).body;
    expect(body).toBe(releaseBody(sha, version));
    expect(body).not.toContain("display session");
    expect(body).not.toContain("waive");
  }));

  it("rejects the same evidence without a waiver", async () => {
    const { calls, fetchImpl } = waiverFetch();
    await expect(promote(fetchImpl)).rejects.toThrow("No successful full installed nightly");
    expect(calls.find(({ url }) => url.includes("/actions/runs?")).url).toContain("status=success");
    expect(calls.some(({ options }) => options.method === "POST")).toBe(false);
  });

  it("keeps a failed nightly status blocking CI without a waiver or for another job", async () => {
    const statuses = [{ context: "CI / smoke (push)", status: "success" }, { context: "Nightly desktop release / macos-e2e (workflow_dispatch)", status: "failure" }];
    await expect(promote(waiverFetch({ statuses }).fetchImpl)).rejects.toThrow("CI is not green");
    await expect(promote(waiverFetch({ statuses, tasks: waivedJobs("linux-e2e") }).fetchImpl, undefined, { e2eWaiver: { job: "linux-e2e", reason: "r" } })).rejects.toThrow("CI is not green");
    await expect(promote(waiverFetch({ statuses: [...statuses, { context: "CI / security (push)", status: "failure" }] }).fetchImpl, undefined, { e2eWaiver: macosWaiver })).rejects.toThrow("CI is not green");
  });

  it.each(["linux-e2e", "windows-e2e", "macos-e2e"])("accepts each allowed waived job: %s", async (job) => {
    for (const status of ["failure", "success"]) {
      const { fetchImpl } = waiverFetch({ tasks: waivedJobs(job, status) });
      await expect(assertInstalledNightly(fetchImpl, "repo-token", forgejo, sha, { job, reason: "r" })).resolves.toBeUndefined();
    }
  });

  it.each(["skipped", "cancelled", "running", "waiting", "blocked", undefined])("rejects a waived job with status %s", async (status) => {
    const tasks = status === undefined ? waivedJobs("macos-e2e").filter(({ name }) => name !== "macos-e2e") : waivedJobs("macos-e2e", status);
    const { calls, fetchImpl } = waiverFetch({ tasks });
    await expect(promote(fetchImpl, undefined, { e2eWaiver: macosWaiver })).rejects.toThrow("satisfies the macos-e2e waiver");
    expect(calls.some(({ options }) => options.method === "POST")).toBe(false);
  });

  it.each(["failure", "cancelled", undefined])("rejects a verify-requested-e2e job with status %s", async (status) => {
    const tasks = status === undefined ? waivedJobs("macos-e2e").filter(({ name }) => name !== "verify-requested-e2e") : waivedJobs("macos-e2e", "failure", { "verify-requested-e2e": status });
    await expect(assertInstalledNightly(waiverFetch({ tasks }).fetchImpl, "repo-token", forgejo, sha, macosWaiver)).rejects.toThrow("waiver");
  });

  it("accepts a successful verify-requested-e2e job", async () => {
    const tasks = waivedJobs("macos-e2e", "failure", { "verify-requested-e2e": "success" });
    await expect(assertInstalledNightly(waiverFetch({ tasks }).fetchImpl, "repo-token", forgejo, sha, macosWaiver)).resolves.toBeUndefined();
  });

  it.each(["linux-e2e", "windows-e2e"])("rejects a second failed E2E job %s", async (second) => {
    const tasks = waivedJobs("macos-e2e", "failure", { [second]: "failure" });
    await expect(promote(waiverFetch({ tasks }).fetchImpl, undefined, { e2eWaiver: macosWaiver })).rejects.toThrow("waiver");
  });

  it.each(["build (linux)", "build (windows)", "build (macos)", "publish"])("rejects a red %s even with a waiver", async (red) => {
    for (const status of ["failure", "skipped", "cancelled"]) {
      const tasks = waivedJobs("macos-e2e", "failure", { [red]: status });
      await expect(assertInstalledNightly(waiverFetch({ tasks }).fetchImpl, "repo-token", forgejo, sha, macosWaiver)).rejects.toThrow("waiver");
    }
    const missing = waivedJobs("macos-e2e").filter(({ name }) => name !== red);
    await expect(assertInstalledNightly(waiverFetch({ tasks: missing }).fetchImpl, "repo-token", forgejo, sha, macosWaiver)).rejects.toThrow("waiver");
  });

  it("considers only success or failure runs at the exact requested SHA", async () => {
    for (const run of [failedRun({ status: "cancelled" }), failedRun({ status: "running" }), failedRun({ commit_sha: "b".repeat(40) }), failedRun({ workflow_id: "ci.yml" })]) {
      await expect(assertInstalledNightly(waiverFetch({ runs: [run] }).fetchImpl, "repo-token", forgejo, sha, macosWaiver)).rejects.toThrow("waiver");
    }
    const otherSha = waivedJobs("macos-e2e").map((entry) => ({ ...entry, head_sha: "b".repeat(40) }));
    await expect(assertInstalledNightly(waiverFetch({ tasks: otherSha }).fetchImpl, "repo-token", forgejo, sha, macosWaiver)).rejects.toThrow("waiver");
    const otherRun = waivedJobs("macos-e2e").map((entry) => ({ ...entry, run_number: 100 }));
    await expect(assertInstalledNightly(waiverFetch({ tasks: otherRun }).fetchImpl, "repo-token", forgejo, sha, macosWaiver)).rejects.toThrow("waiver");
    const split = [...waivedJobs("macos-e2e", "failure", { "linux-e2e": "failure" }), ...waivedJobs("macos-e2e", "failure", { "windows-e2e": "failure" }).map((entry) => ({ ...entry, run_number: 100 }))];
    await expect(assertInstalledNightly(waiverFetch({ runs: [failedRun(), failedRun({ index_in_repo: 100 })], tasks: split }).fetchImpl, "repo-token", forgejo, sha, macosWaiver)).rejects.toThrow("waiver");
  });

  it("judges waived evidence by the newest task attempt", async () => {
    const retried = (newest, older) => waiverFetch({ tasks: [task("linux-e2e", { status: newest }), ...waivedJobs("macos-e2e", "failure", { "linux-e2e": older })] });
    await expect(assertInstalledNightly(retried("success", "failure").fetchImpl, "repo-token", forgejo, sha, macosWaiver)).resolves.toBeUndefined();
    await expect(assertInstalledNightly(retried("failure", "success").fetchImpl, "repo-token", forgejo, sha, macosWaiver)).rejects.toThrow("waiver");
    const waivedRetry = waiverFetch({ tasks: [task("macos-e2e", { status: "cancelled" }), ...waivedJobs("macos-e2e", "failure")] });
    await expect(assertInstalledNightly(waivedRetry.fetchImpl, "repo-token", forgejo, sha, macosWaiver)).rejects.toThrow("waiver");
  });

  it("keeps a dry run free of mutations and records no waiver when none is given", () => withSummary(async (summaryFile) => {
    const { calls, fetchImpl } = waiverFetch();
    const lines = [];
    await promote(fetchImpl, undefined, { dryRun: true, e2eWaiver: macosWaiver, summaryFile, log: (line) => lines.push(line) });
    expect(calls.filter(({ options }) => ["POST", "PATCH", "DELETE"].includes(options.method))).toEqual([]);
    expect(lines.some((line) => line.startsWith("dry run:"))).toBe(true);
    expect(readFileSync(summaryFile, "utf8")).toContain("macos-e2e");
    writeFileSync(summaryFile, "");
    const clean = [];
    await promote(promotionFetch().fetchImpl, undefined, { dryRun: true, summaryFile, log: (line) => clean.push(line) });
    expect(readFileSync(summaryFile, "utf8")).toBe("");
    expect(clean.join("\n")).not.toContain("waiver");
  }));

  it("reads the waiver from the CLI environment and validates it before any request", () => {
    const directory = mkdtempSync(path.join(tmpdir(), "muniment-waiver-cli-"));
    try {
      const preload = path.join(directory, "fetch.mjs");
      writeFileSync(preload, "globalThis.fetch = async () => { throw new Error('FETCH_CALLED'); };");
      const run = (env, mode = []) => execFileSync(process.execPath, ["--import", preload, path.resolve(".github/lib/release-promotion.mjs"), "owner/repo", sha, version, ...mode], {
        env: { ...process.env, GH_TOKEN: "t", REPO_TOKEN: "t", REPO_API: forgejo, E2E_WAIVER_JOB: "", E2E_WAIVER_REASON: "", ...env }, stdio: "pipe", timeout: 10000,
      });
      expect(() => run({ E2E_WAIVER_JOB: "macos-e2e", E2E_WAIVER_REASON: "  " })).toThrow(/requires a reason/);
      expect(() => run({ E2E_WAIVER_JOB: "macos-e2e", E2E_WAIVER_REASON: "  " }, ["--dry-run"])).toThrow(/requires a reason/);
      expect(() => run({ E2E_WAIVER_JOB: "smoke", E2E_WAIVER_REASON: "r" })).toThrow(/must be one of/);
      expect(() => run({ E2E_WAIVER_JOB: "macos-e2e", E2E_WAIVER_REASON: "r" }, ["--dry-run"])).toThrow(/FETCH_CALLED/);
      expect(() => run({})).toThrow(/FETCH_CALLED/);
    } finally { rmSync(directory, { recursive: true, force: true }); }
  });
});
