import { readdir, readFile } from "node:fs/promises";
import { basename, join } from "node:path";

const [token, repository, sha, platform] = process.argv.slice(2);
if (!token || !repository || !/^[0-9a-f]{40}$/.test(sha) || !platform) {
  throw new Error("usage: upload-nightly-assets.mjs <token> <owner/repo> <sha> <platform>");
}

const specs = {
  linux: [["deb", ".deb"], ["appimage", ".AppImage"]],
  windows: [["msi", ".msi", "-machine.msi"], ["msi", "-machine.msi"], ["nsis", "-setup.exe"]],
  macos: [["macos", ".app.zip"]],
};
if (!specs[platform]) throw new Error(`unsupported platform: ${platform}`);

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
for (const [directory, suffix, excludeSuffix] of specs[platform]) {
  const bundleDirectory = join(bundleBase, directory);
  const matches = (await readdir(bundleDirectory)).filter(
    (name) => name.endsWith(suffix) && (!excludeSuffix || !name.endsWith(excludeSuffix)),
  );
  if (matches.length !== 1) {
    throw new Error(`expected one ${suffix} in ${bundleDirectory}, found: ${matches.join(", ") || "none"}`);
  }

  const path = join(bundleDirectory, matches[0]);
  const assetName = `nightly-${sha}-${platform}-${basename(path).replace("-setup.exe", "-nsis.exe")}`;
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
