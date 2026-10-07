import { describe, expect, it, vi } from "vitest";
import { githubReleases } from "./github-releases.mjs";
import { ensureNightlyRelease, finalizeNightlyRelease, waitForMirroredCommit } from "./nightly-release.mjs";
import { macosSigningProvenance, windowsSigningProvenance } from "./release-promotion.mjs";

const sha = "a".repeat(40);
const owner = "munimentai";
const repo = "muniment";
const notFound = () => Object.assign(new Error("missing"), { status: 404 });
const names = [
  "linux-muniment.deb", "linux-muniment_0.1.0_amd64.AppImage", "linux-muniment_0.1.0_amd64.AppImage.sig",
  "windows-muniment_0.1.0_x64_en-US.msi", "windows-muniment_0.1.0_x64_en-US.msi.sig",
  "windows-muniment_0.1.0_x64_en-US-machine.msi", "windows-muniment_0.1.0_x64_en-US-machine.msi.sig",
  "windows-muniment_0.1.0_x64-nsis.exe", "windows-muniment_0.1.0_x64-nsis.exe.sig",
  ...["", "-arm64", "-x64"].flatMap(arch => [".app.zip", ".pkg", ".dmg", ".app.tar.gz", ".app.tar.gz.sig"].map(format => `macos-muniment${arch}${format}`)),
];
const current = names.map((name, index) => ({ id: index + 1, name: `nightly-${sha}-${name}` }));

const fakeGithub = ({ release, ref = true, commits = [] } = {}) => ({
  rest: {
    git: { getRef: vi.fn(async () => { if (!ref) throw notFound(); return { data: { object: { sha } } }; }) },
    repos: {
      getCommit: vi.fn(async () => { const next = commits.shift(); if (next instanceof Error) throw next; return { data: { sha: next } }; }),
      getReleaseByTag: vi.fn(async () => { if (!release) throw notFound(); return { data: release }; }),
      createRelease: vi.fn(async () => ({ data: { id: 7 } })),
      updateRelease: vi.fn(async () => ({ data: {} })),
      deleteReleaseAsset: vi.fn(async () => ({ data: undefined })),
    },
  },
});

describe("rolling nightly pre-release", () => {
  it("keeps an existing release", async () => {
    const github = fakeGithub({ release: { id: 1, assets: [] } });
    await ensureNightlyRelease({ github, owner, repo });
    expect(github.rest.repos.createRelease).not.toHaveBeenCalled();
  });

  it("creates the release on the mirrored tag without naming a target", async () => {
    const github = fakeGithub();
    await ensureNightlyRelease({ github, owner, repo });
    expect(github.rest.repos.createRelease).toHaveBeenCalledWith({
      owner, repo, tag_name: "nightly", name: "Nightly desktop build", body: "Initial nightly build is in progress.", prerelease: true,
    });
  });

  it("refuses to create a GitHub-only tag", async () => {
    const github = fakeGithub({ ref: false });
    await expect(ensureNightlyRelease({ github, owner, repo })).rejects.toThrow("Create the nightly tag on Forgejo");
    expect(github.rest.repos.createRelease).not.toHaveBeenCalled();
  });

  it("waits for the push mirror to carry the source", async () => {
    const wait = vi.fn(async () => {});
    const github = fakeGithub({ commits: [notFound(), notFound(), sha] });
    await waitForMirroredCommit({ github, owner, repo, sha, wait, log: () => {} });
    expect(wait).toHaveBeenCalledTimes(2);
    const missing = fakeGithub({ commits: [notFound(), notFound()] });
    await expect(waitForMirroredCommit({ github: missing, owner, repo, sha, attempts: 2, wait, log: () => {} })).rejects.toThrow("push mirror");
    const failing = fakeGithub({ commits: [Object.assign(new Error("denied"), { status: 401 })] });
    await expect(waitForMirroredCommit({ github: failing, owner, repo, sha, wait, log: () => {} })).rejects.toThrow("denied");
  });

  it("finalizes one full asset set and removes every other asset", async () => {
    const stale = { id: 99, name: `nightly-${"b".repeat(40)}-linux-muniment.deb` };
    const github = fakeGithub({ release: { id: 5, assets: [...current, stale] } });
    await finalizeNightlyRelease({ github, owner, repo, sha, windowsSigning: true, macosSigning: true, now: new Date("2026-10-07T12:34:56.789Z") });
    expect(github.rest.repos.deleteReleaseAsset).toHaveBeenCalledTimes(1);
    expect(github.rest.repos.deleteReleaseAsset).toHaveBeenCalledWith({ owner, repo, asset_id: 99 });
    const update = github.rest.repos.updateRelease.mock.calls[0][0];
    expect(update).toMatchObject({ owner, repo, release_id: 5, name: "Nightly desktop build", prerelease: true });
    expect(update).not.toHaveProperty("target_commitish");
    expect(update.body).toContain(`Automated desktop build from \`${sha}\`.`);
    expect(update.body).toContain("Built 2026-10-07 12:34 UTC.");
    expect(update.body).toContain(windowsSigningProvenance(sha));
    expect(update.body).toContain(macosSigningProvenance(sha));
  });

  it("records unsigned builds", async () => {
    const github = fakeGithub({ release: { id: 5, assets: current } });
    await finalizeNightlyRelease({ github, owner, repo, sha, windowsSigning: false, macosSigning: false });
    const { body } = github.rest.repos.updateRelease.mock.calls[0][0];
    expect(body).toContain("Windows installers are unsigned; stable promotion is blocked.");
    expect(body).toContain("macOS artifacts are unsigned pending Apple enrollment Y5DUNHQA74.");
  });

  it.each(["linux-muniment.deb", "macos-muniment-x64.app.tar.gz.sig", "windows-muniment_0.1.0_x64_en-US-machine.msi"])("fails without %s", async (name) => {
    const github = fakeGithub({ release: { id: 5, assets: current.filter((asset) => !asset.name.endsWith(name)) } });
    await expect(finalizeNightlyRelease({ github, owner, repo, sha })).rejects.toThrow("expected one");
    expect(github.rest.repos.deleteReleaseAsset).not.toHaveBeenCalled();
    expect(github.rest.repos.updateRelease).not.toHaveBeenCalled();
  });
});

describe("GitHub Releases client", () => {
  it("sends the release token to GitHub with one Accept header per request", async () => {
    const calls = [];
    const github = githubReleases("fixture-token", async (url, options) => {
      calls.push({ url, options });
      return new Response(url.includes("/assets/3") ? "bytes" : "{}", { status: 200 });
    });
    const { data } = await github.rest.repos.getReleaseAsset({ owner, repo, asset_id: 3, headers: { accept: "application/octet-stream" } });
    expect(data.toString()).toBe("bytes");
    await github.rest.repos.uploadReleaseAsset({ owner, repo, release_id: 5, name: "a b.sig", data: "sig" });
    expect(calls[0].url).toBe("https://api.github.com/repos/munimentai/muniment/releases/assets/3");
    expect(calls[0].options.headers.get("accept")).toBe("application/octet-stream");
    expect(calls[0].options.headers.get("authorization")).toBe("Bearer fixture-token");
    expect(calls[1].url).toBe("https://uploads.github.com/repos/munimentai/muniment/releases/5/assets?name=a%20b.sig");
    expect(calls[1].options.headers.get("content-type")).toBe("application/octet-stream");
  });

  it("reports the HTTP status of a failed call", async () => {
    const github = githubReleases("fixture-token", async () => new Response("missing", { status: 404 }));
    await expect(github.rest.repos.getReleaseByTag({ owner, repo, tag: "nightly" })).rejects.toMatchObject({ status: 404 });
    expect(() => githubReleases("")).toThrow("token");
  });
});
