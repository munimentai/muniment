import { readFile } from "node:fs/promises";
import { githubReleases } from "./lib/github-releases.mjs";
import { prepareUpdateArtifacts } from "./lib/update-artifacts.mjs";
import { basename, join } from "node:path";

const token = process.env.GH_TOKEN;
const [repository, sha, platform] = process.argv.slice(2);
if (!token || !repository || !/^[0-9a-f]{40}$/.test(sha) || !platform) {
  throw new Error("usage: GH_TOKEN=<injected> upload-nightly-assets.mjs <owner/repo> <sha> <platform>");
}
const [owner, repo] = repository.split("/");
const github = githubReleases(token);

// macOS builds a universal (x86_64+arm64) app, which tauri writes under the
// universal-apple-darwin target dir rather than the default release dir.
const bundleBase = platform === "macos"
  ? join("src-tauri", "target", "universal-apple-darwin", "release", "bundle")
  : join("src-tauri", "target", "release", "bundle");

const { data: release } = await github.rest.repos.getReleaseByTag({ owner, repo, tag: "nightly" });
for (const path of await prepareUpdateArtifacts(platform, bundleBase)) {
  const assetName = platform === "linux" && path.endsWith(".deb")
    ? `nightly-${sha}-linux-muniment.deb`
    : `nightly-${sha}-${platform}-${basename(path).replace("-setup.exe", "-nsis.exe")}`;
  const old = release.assets.find((asset) => asset.name === assetName);
  if (old) await github.rest.repos.deleteReleaseAsset({ owner, repo, asset_id: old.id });
  await github.rest.repos.uploadReleaseAsset({ owner, repo, release_id: release.id, name: assetName, data: await readFile(path) });
  console.log(`uploaded ${assetName}`);
}
