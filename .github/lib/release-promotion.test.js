import { execFileSync } from "node:child_process";
import { createHash, generateKeyPairSync } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it, vi } from "vitest";
import {
  APPLE_TEAM_ID, assertGreenCi, expectedNightlyAssets, macosSigningProvenance, microsoftRootPem, promoteRelease, releaseBody,
  sha256Sums, stableAssetName, validatePromotionInputs, verifyAuthenticode, verifyMacosApp, verifyMacosPackage,
  verifyNightlyArtifacts, WINDOWS_PUBLISHER, windowsSigningProvenance,
} from "./release-promotion.mjs";
import { signUpdaterBytes } from "./updater-signature.mjs";

const sha = "a".repeat(40);
const version = "v0.0.1";
const assetNames = [
  `nightly-${sha}-linux-muniment.deb`,
  `nightly-${sha}-linux-muniment.AppImage`,
  `nightly-${sha}-windows-muniment.msi`,
  `nightly-${sha}-windows-muniment-machine.msi`,
  `nightly-${sha}-windows-muniment-nsis.exe`,
  `nightly-${sha}-macos-muniment.app.zip`,
  `nightly-${sha}-macos-muniment.pkg`,
  `nightly-${sha}-macos-muniment.app.tar.gz`,
  `nightly-${sha}-macos-muniment.app.tar.gz.sig`,
  `nightly-${sha}-linux-muniment.AppImage.sig`,
  `nightly-${sha}-windows-muniment.msi.sig`,
  `nightly-${sha}-windows-muniment-machine.msi.sig`,
  `nightly-${sha}-windows-muniment-nsis.exe.sig`,
];
const assets = assetNames.map((name, id) => ({ id, name, url: `https://api.github.test/assets/${id}`, content_type: "application/octet-stream" }));
const smoke = { name: "smoke", status: "completed", conclusion: "success" };

const response = (body, status = 200, headers = {}) => new Response(
  typeof body === "string" ? body : JSON.stringify(body),
  { status, headers: { "Content-Type": "application/json", ...headers } },
);

const promotionFetch = (overrides = {}) => {
  const calls = [];
  const fetchImpl = async (url, options = {}) => {
    calls.push({ url, options });
    const method = options.method ?? "GET";
    if (overrides.route) {
      const result = await overrides.route(url, method, options, calls);
      if (result) return result;
    }
    if (url.includes(`/git/ref/tags/${version}`) || url.includes(`/releases/tags/${version}`)) return response("missing", 404);
    if (url.includes("/contents/package.json")) return response({ content: Buffer.from(JSON.stringify({ version: version.slice(1) })).toString("base64") });
    if (url.includes("/actions/workflows/nightly.yml/runs")) return response({ workflow_runs: [{ id: 99, head_sha: sha, status: "completed", conclusion: "success" }] });
    if (url.includes("/actions/runs/99/jobs")) return response({ jobs: ["build (linux)", "build (windows)", "build (macos)", "publish", "linux-e2e", "windows-e2e", "macos-e2e"].map((name) => ({ ...smoke, name })) });
    if (url.includes("/check-runs")) return response({ check_runs: [smoke] });
    if (url.endsWith("/releases/tags/nightly")) return response({
      id: 1, draft: false, prerelease: true, target_commitish: overrides.targetCommitish ?? "stale-branch-value",
      body: `Automated desktop build from ${sha}.\n\n${windowsSigningProvenance(sha)}${overrides.macosSigned !== false ? `\n\n${macosSigningProvenance(sha)}` : "\n\nmacOS artifacts are unsigned pending Apple enrollment Y5DUNHQA74."}`, assets,
    });
    if (url.endsWith("/releases") && method === "POST") return response({ id: 42 });
    if (url.startsWith("https://api.github.test/assets/")) return response(assets[Number(url.split("/").at(-1))]?.name.endsWith(".sig") ? Buffer.from("signature fixture").toString("base64") : "asset bytes");
    if (url.startsWith("https://uploads.github.com/")) return response({});
    if (url.endsWith("/releases/42") && ["PATCH", "DELETE"].includes(method)) return response({});
    if (url.includes(`/git/refs/tags/${version}`) && method === "DELETE") return response({});
    throw new Error(`unexpected request: ${method} ${url}`);
  };
  return { calls, fetchImpl };
};

// The promotion flow tests inject the artifact verifier; the verifier's own
// checks are covered below against tool output fixtures.
const promote = (fetchImpl, verify = async () => {}) => promoteRelease({ token: "token", repository: "owner/repo", sha, version, fetchImpl, verify });

describe("stable release promotion", () => {
  it("accepts only exact lowercase SHAs and strict stable SemVer tags", () => {
    expect(() => validatePromotionInputs(sha, "v1.2.3")).not.toThrow();
    for (const invalid of ["1.2.3", "v1.2", "v01.2.3", "v1.2.3-beta", "v1.2.3 "]) expect(() => validatePromotionInputs(sha, invalid)).toThrow("strict SemVer");
    for (const invalid of ["a".repeat(39), "A".repeat(40), `${sha}0`, "g".repeat(40)]) expect(() => validatePromotionInputs(invalid, "v1.2.3")).toThrow("40 lowercase");
  });

  it("requires exactly one of each finalized nightly artifact", () => {
    expect(expectedNightlyAssets(assets, sha)).toEqual(assets);
    expect(() => expectedNightlyAssets(assets.slice(1), sha)).toThrow("exactly thirteen");
    expect(() => expectedNightlyAssets([...assets.slice(0, 12), assets[0]], sha)).toThrow();
  });

  it("requires smoke and rejects pending or otherwise-named failed checks", () => {
    expect(() => assertGreenCi([smoke], sha)).not.toThrow();
    expect(() => assertGreenCi([], sha)).toThrow("CI is not green");
    expect(() => assertGreenCi([{ ...smoke, status: "in_progress", conclusion: null }], sha)).toThrow("CI is not green");
    expect(() => assertGreenCi([smoke, { name: "promote", status: "in_progress", conclusion: null }], sha)).not.toThrow();
    expect(() => assertGreenCi([smoke, { name: "security", status: "completed", conclusion: "failure" }], sha)).toThrow("CI is not green");
  });

  it("publishes signing provenance for both platforms", () => {
    const body = releaseBody(sha, version);
    expect(body).toContain(sha);
    expect(body).toContain("Windows installers are signed");
    expect(body).toContain(macosSigningProvenance(sha));
    expect(body).not.toContain("unsigned");
    expect(body).toContain("Model weights are not included");
  });

  it.each([["tag", `/git/ref/tags/${version}`], ["release", `/releases/tags/${version}`]])("rejects an existing %s", async (kind, endpoint) => {
    const { fetchImpl } = promotionFetch({ route: (url) => url.includes(endpoint) ? response({}) : null });
    await expect(promote(fetchImpl)).rejects.toThrow(`${kind} ${version} already exists`);
  });

  it("rejects a source-version mismatch before inspecting nightly", async () => {
    const { calls, fetchImpl } = promotionFetch({ route: (url) => url.includes("/contents/package.json") ? response({ content: Buffer.from('{"version":"9.9.9"}').toString("base64") }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("does not match");
    expect(calls.some(({ url }) => url.endsWith("/releases/tags/nightly"))).toBe(false);
  });

  it("rejects a later-page CI failure", async () => {
    const successes = Array.from({ length: 100 }, (_, index) => index === 0 ? smoke : { name: `check-${index}`, status: "completed", conclusion: "success" });
    const { fetchImpl } = promotionFetch({ route: (url) => url.includes("check-runs") ? response({ check_runs: new URL(url).searchParams.get("page") === "1" ? successes : [{ name: "late failure", status: "completed", conclusion: "failure" }] }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("CI is not green");
  });

  it("uses the nightly tag despite stale target_commitish and copies exactly thirteen assets without mutating nightly", async () => {
    const { calls, fetchImpl } = promotionFetch();
    await promote(fetchImpl);
    const create = calls.find(({ url, options }) => url.endsWith("/releases") && options.method === "POST");
    expect(JSON.parse(create.options.body)).toMatchObject({ tag_name: version, target_commitish: sha, draft: true, prerelease: false });
    expect(calls.filter(({ url }) => url.startsWith("https://api.github.test/assets/"))).toHaveLength(13);
    expect(calls.filter(({ url }) => url.startsWith("https://uploads.github.com/"))).toHaveLength(15);
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
    expect(sums.trim().split("\n")).toHaveLength(14);
    expect(sums).toContain(`${digest("asset bytes")}  muniment-0.0.1-macos.pkg\n`);
    expect(sums).toContain(`${digest(feedBody)}  latest.json\n`);
    for (const upload of uploads.filter(({ url }) => !url.endsWith("name=latest.json") && !url.endsWith("name=SHA256SUMS"))) {
      const name = new URL(upload.url).searchParams.get("name");
      expect(Buffer.from(upload.options.body).toString()).toBe(name.endsWith(".sig") ? Buffer.from("signature fixture").toString("base64") : "asset bytes");
    }
    const publish = calls.find(({ url, options }) => url.endsWith("/releases/42") && options.method === "PATCH");
    expect(JSON.parse(publish.options.body)).toEqual({ draft: false, prerelease: false });
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
    const { calls, fetchImpl } = promotionFetch({ route: (url) => url.includes("/actions/runs/99/jobs") ? response({ jobs: ["build (linux)", "build (windows)", "build (macos)", "publish", "linux-e2e", "windows-e2e", "macos-e2e"].filter((name) => name !== missing).map((name) => ({ ...smoke, name })) }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("No successful full installed nightly");
    expect(calls.some(({ options }) => options.method === "POST")).toBe(false);
  });

  it.each(["failure", "skipped"])("rejects an installed check with conclusion %s", async (conclusion) => {
    const { fetchImpl } = promotionFetch({ route: (url) => url.includes("/actions/runs/99/jobs") ? response({ jobs: ["build (linux)", "build (windows)", "build (macos)", "publish", "linux-e2e", "windows-e2e", "macos-e2e"].map((name) => ({ ...smoke, name, conclusion: name === "macos-e2e" ? conclusion : "success" })) }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("No successful full installed nightly");
  });

  it("rejects a successful nightly for another revision", async () => {
    const { fetchImpl } = promotionFetch({ route: (url) => url.includes("/actions/workflows/nightly.yml/runs") ? response({ workflow_runs: [{ id: 99, head_sha: "b".repeat(40), status: "completed", conclusion: "success" }] }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("No successful full installed nightly");
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
    expect(calls.some(({ url, options }) => url.includes(`/git/refs/tags/${version}`) && options.method === "DELETE")).toBe(true);
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
      env: { ...process.env, GH_TOKEN: "fixture-only-release-token" }, stdio: "pipe", timeout: 10000,
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
