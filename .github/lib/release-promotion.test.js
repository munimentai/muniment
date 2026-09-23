import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { assertGreenCi, expectedNightlyAssets, macosSigningProvenance, promoteRelease, releaseBody, stableAssetName, validatePromotionInputs, windowsSigningProvenance } from "./release-promotion.mjs";

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

const promote = (fetchImpl) => promoteRelease({ token: "token", repository: "owner/repo", sha, version, fetchImpl });

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
    expect(calls.filter(({ url }) => url.startsWith("https://uploads.github.com/"))).toHaveLength(14);
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
    for (const upload of uploads.filter(({ url }) => !url.endsWith("name=latest.json"))) {
      const name = new URL(upload.url).searchParams.get("name");
      expect(Buffer.from(upload.options.body).toString()).toBe(name.endsWith(".sig") ? Buffer.from("signature fixture").toString("base64") : "asset bytes");
    }
    const publish = calls.find(({ url, options }) => url.endsWith("/releases/42") && options.method === "PATCH");
    expect(JSON.parse(publish.options.body)).toEqual({ draft: false, prerelease: false });
  });

  it("rejects unsigned macOS artifacts before creating a release", async () => {
    const { calls, fetchImpl } = promotionFetch({ macosSigned: false });
    await expect(promote(fetchImpl)).rejects.toThrow("macOS artifacts are not verified as signed and notarized");
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

  it("fails closed when finalized nightly signing provenance is absent", async () => {
    const { calls, fetchImpl } = promotionFetch({ route: (url) => url.endsWith("/releases/tags/nightly") ? response({ draft: false, prerelease: true, body: `Built from ${sha}`, assets }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("not verified as signed");
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
