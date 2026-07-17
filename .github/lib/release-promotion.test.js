import { describe, expect, it } from "vitest";
import { assertGreenCi, expectedNightlyAssets, promoteRelease, releaseBody, validatePromotionInputs, windowsSigningProvenance } from "./release-promotion.mjs";

const sha = "a".repeat(40);
const version = "v0.0.1";
const assetNames = [
  `nightly-${sha}-linux-muniment.deb`,
  `nightly-${sha}-linux-muniment.AppImage`,
  `nightly-${sha}-windows-muniment.msi`,
  `nightly-${sha}-windows-muniment-machine.msi`,
  `nightly-${sha}-windows-muniment-nsis.exe`,
  `nightly-${sha}-macos-muniment.app.zip`,
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
    if (url.includes("/check-runs")) return response({ check_runs: [smoke] });
    if (url.endsWith("/releases/tags/nightly")) return response({
      id: 1, draft: false, prerelease: true, target_commitish: overrides.targetCommitish ?? "stale-branch-value",
      body: `Automated desktop build from ${sha}.\n\n${windowsSigningProvenance(sha)}`, assets,
    });
    if (url.endsWith("/git/ref/tags/nightly")) return response({ object: { sha } });
    if (url.endsWith("/releases") && method === "POST") return response({ id: 42 });
    if (url.startsWith("https://api.github.test/assets/")) return response("asset bytes");
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
    expect(() => expectedNightlyAssets(assets.slice(1), sha)).toThrow("exactly six");
    expect(() => expectedNightlyAssets([...assets.slice(0, 5), assets[0]], sha)).toThrow("Linux deb");
  });

  it("requires smoke and rejects pending or otherwise-named failed checks", () => {
    expect(() => assertGreenCi([smoke], sha)).not.toThrow();
    expect(() => assertGreenCi([], sha)).toThrow("CI is not green");
    expect(() => assertGreenCi([{ ...smoke, status: "in_progress", conclusion: null }], sha)).toThrow("CI is not green");
    expect(() => assertGreenCi([smoke, { name: "promote", status: "in_progress", conclusion: null }], sha)).not.toThrow();
    expect(() => assertGreenCi([smoke, { name: "security", status: "completed", conclusion: "failure" }], sha)).toThrow("CI is not green");
  });

  it("publishes the required provenance and signing disclosures", () => {
    const body = releaseBody(sha);
    expect(body).toContain(sha);
    expect(body).toContain("Windows installers are signed");
    expect(body).toContain("macOS artifacts are unsigned pending Apple credentials");
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
    const { fetchImpl } = promotionFetch({ route: (url) => url.includes("check-runs") ? response({ check_runs: url.includes("page=1") ? successes : [{ name: "late failure", status: "completed", conclusion: "failure" }] }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("CI is not green");
  });

  it("uses the nightly tag despite stale target_commitish and copies exactly six assets without mutating nightly", async () => {
    const { calls, fetchImpl } = promotionFetch();
    await promote(fetchImpl);
    const create = calls.find(({ url, options }) => url.endsWith("/releases") && options.method === "POST");
    expect(JSON.parse(create.options.body)).toMatchObject({ tag_name: version, target_commitish: sha, draft: true, prerelease: false });
    expect(calls.filter(({ url }) => url.startsWith("https://api.github.test/assets/"))).toHaveLength(6);
    expect(calls.filter(({ url }) => url.startsWith("https://uploads.github.com/"))).toHaveLength(6);
    expect(calls.some(({ url, options }) => url.includes("/releases/1") && options.method)).toBe(false);
    const publish = calls.find(({ url, options }) => url.endsWith("/releases/42") && options.method === "PATCH");
    expect(JSON.parse(publish.options.body)).toEqual({ draft: false, prerelease: false });
  });

  it("rejects a nightly tag mismatch", async () => {
    const { fetchImpl } = promotionFetch({ route: (url) => url.endsWith("/git/ref/tags/nightly") ? response({ object: { sha: "b".repeat(40) } }) : null });
    await expect(promote(fetchImpl)).rejects.toThrow("not finalized");
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
