import { describe, expect, it, vi } from "vitest";
import { githubReleases } from "./github-releases.mjs";
import { candidateTag, findCandidateDraft } from "./candidate-release.mjs";

const sha = "a".repeat(40);
const other = "b".repeat(40);
const owner = "munimentai";
const repo = "muniment";
const filler = (count, prefix) => Array.from({ length: count }, (_, index) => ({ id: index, tag_name: `${prefix}${index}`, draft: false }));
const draft = (id, tag = `candidate-${sha}`) => ({ id, tag_name: tag, draft: true });
const fakeGithub = (pages) => {
  const listReleases = vi.fn(async ({ page }) => {
    const next = pages[page - 1] ?? [];
    if (next instanceof Error) throw next;
    return { data: next };
  });
  return { rest: { repos: { listReleases } } };
};
const find = (github, options = {}) => findCandidateDraft({ github, owner, repo, sha, perPage: 3, ...options });

describe("candidate draft lookup", () => {
  it("finds a draft on a later page", async () => {
    const github = fakeGithub([filler(3, "v"), filler(3, "w"), [{ id: 1, tag_name: "nightly", draft: false }, draft(42)]]);
    await expect(find(github)).resolves.toMatchObject({ id: 42, draft: true });
    expect(github.rest.repos.listReleases.mock.calls.map(([args]) => args.page)).toEqual([1, 2, 3]);
    expect(github.rest.repos.listReleases).toHaveBeenCalledWith({ owner, repo, per_page: 3, page: 1 });
  });

  it("ignores public, stable, nightly and other-SHA releases", async () => {
    const github = fakeGithub([[
      { id: 1, tag_name: "nightly", draft: false },
      { id: 2, tag_name: "v1.0.0", draft: false },
      { id: 3, tag_name: `candidate-${other}`, draft: true },
    ], [draft(4, `candidate-${sha}-extra`), draft(5)]]);
    await expect(find(github)).resolves.toMatchObject({ id: 5 });
  });

  it("fails when the candidate is missing instead of choosing another release", async () => {
    const github = fakeGithub([[{ id: 1, tag_name: "nightly", draft: false }, draft(2, `candidate-${other}`)]]);
    await expect(find(github)).rejects.toThrow(`no draft release candidate-${sha}`);
    await expect(find(fakeGithub([]))).rejects.toThrow("no draft release");
  });

  it("rejects a published release with the candidate tag", async () => {
    const github = fakeGithub([[{ id: 1, tag_name: `candidate-${sha}`, draft: false }]]);
    await expect(find(github)).rejects.toThrow("not a draft");
  });

  it("rejects a malformed SHA before calling GitHub", async () => {
    const github = fakeGithub([]);
    for (const bad of ["", "abc", "A".repeat(40), "a".repeat(39), "a".repeat(41), "g".repeat(40), undefined, null, 5]) {
      await expect(find(github, { sha: bad })).rejects.toThrow("40-character");
    }
    expect(() => candidateTag("nightly")).toThrow("40-character");
    expect(github.rest.repos.listReleases).not.toHaveBeenCalled();
  });

  it("rejects duplicate matching releases, including across pages", async () => {
    await expect(find(fakeGithub([[draft(1), draft(2)]]))).rejects.toThrow("2 releases tagged");
    await expect(find(fakeGithub([[draft(1), ...filler(2, "v")], [draft(2)]]))).rejects.toThrow("2 releases tagged");
  });

  it("propagates API and authentication failures", async () => {
    const denied = Object.assign(new Error("bad credentials"), { status: 401 });
    await expect(find(fakeGithub([denied]))).rejects.toBe(denied);
    const late = Object.assign(new Error("rate limited"), { status: 403 });
    await expect(find(fakeGithub([filler(3, "v"), late]))).rejects.toBe(late);
  });

  it("fails when the release list never ends", async () => {
    await expect(find(fakeGithub([filler(3, "a"), filler(3, "b")]), { maxPages: 2 })).rejects.toThrow("after 2 pages");
  });

  it("rejects a response that is not a release list", async () => {
    const github = { rest: { repos: { listReleases: async () => ({ data: { message: "oops" } }) } } };
    await expect(find(github)).rejects.toThrow("array");
  });

  it("lists releases through the authenticated client", async () => {
    const urls = [];
    const client = githubReleases("fixture-token", async (url, options) => {
      urls.push({ url, auth: options.headers.get("authorization") });
      return new Response(JSON.stringify(url.endsWith("page=1") ? [draft(9)] : []), { status: 200 });
    });
    await expect(findCandidateDraft({ github: client, owner, repo, sha, perPage: 1 })).resolves.toMatchObject({ id: 9 });
    expect(urls[0]).toEqual({ url: "https://api.github.com/repos/munimentai/muniment/releases?per_page=1&page=1", auth: "Bearer fixture-token" });
    const failing = githubReleases("fixture-token", async () => new Response("denied", { status: 401 }));
    await expect(findCandidateDraft({ github: failing, owner, repo, sha })).rejects.toMatchObject({ status: 401 });
  });
});
