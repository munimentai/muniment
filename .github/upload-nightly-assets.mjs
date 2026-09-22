import { readFile } from "node:fs/promises";
import { prepareUpdateArtifacts } from "./lib/update-artifacts.mjs";
import { basename, join } from "node:path";

const token = process.env.GH_TOKEN;
const [repository, sha, platform] = process.argv.slice(2);
if (!token || !repository || !/^[0-9a-f]{40}$/.test(sha) || !platform) {
  throw new Error("usage: GH_TOKEN=<injected> upload-nightly-assets.mjs <owner/repo> <sha> <platform>");
}

const api = async (url, options = {}) => {
  const response = await fetch(url, {
    ...options,
    headers: {
      Accept: "application/vnd.github+json",
      Authorization: `Bearer ${token}`,
      "X-GitHub-Api-Version": "2022-11-28",
      ...options.headers,
    },
  });
  if (!response.ok) throw new Error(`${options.method ?? "GET"} ${url}: ${response.status} ${await response.text()}`);
  return response;
};

// macOS builds a universal (x86_64+arm64) app, which tauri writes under the
// universal-apple-darwin target dir rather than the default release dir.
const bundleBase = platform === "macos"
  ? join("src-tauri", "target", "universal-apple-darwin", "release", "bundle")
  : join("src-tauri", "target", "release", "bundle");

const release = await (await api(`https://api.github.com/repos/${repository}/releases/tags/nightly`)).json();
for (const path of await prepareUpdateArtifacts(platform, bundleBase)) {
  const assetName = platform === "linux" && path.endsWith(".deb")
    ? `nightly-${sha}-linux-muniment.deb`
    : `nightly-${sha}-${platform}-${basename(path).replace("-setup.exe", "-nsis.exe")}`;
  const old = release.assets.find((asset) => asset.name === assetName);
  if (old) await api(`https://api.github.com/repos/${repository}/releases/assets/${old.id}`, { method: "DELETE" });

  const uploadUrl = `https://uploads.github.com/repos/${repository}/releases/${release.id}/assets?name=${encodeURIComponent(assetName)}`;
  await api(uploadUrl, {
    method: "POST",
    headers: { "Content-Type": "application/octet-stream" },
    body: await readFile(path),
  });
  console.log(`uploaded ${assetName}`);
}
