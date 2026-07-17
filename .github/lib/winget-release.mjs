import { createWingetManifest, machineMsiAsset } from "./winget-manifest.mjs";

const [token, repository, tag, outputRoot] = process.argv.slice(2);
if (!token || !/^[^/]+\/[^/]+$/.test(repository) || !tag || !outputRoot) {
  throw new Error("usage: winget-release.mjs TOKEN OWNER/REPO TAG OUTPUT_ROOT");
}
const headers = {
  Accept: "application/vnd.github+json",
  Authorization: `Bearer ${token}`,
  "X-GitHub-Api-Version": "2022-11-28",
};
const request = async (url, options = {}) => {
  const response = await fetch(url, { ...options, headers: { ...headers, ...options.headers } });
  if (!response.ok) throw new Error(`${url}: ${response.status} ${await response.text()}`);
  return response;
};
const release = await (await request(`https://api.github.com/repos/${repository}/releases/tags/${encodeURIComponent(tag)}`)).json();
const asset = machineMsiAsset(release);
const installer = new Uint8Array(await (await request(asset.url, { headers: { Accept: "application/octet-stream" } })).arrayBuffer());
const file = await createWingetManifest({ release, installerBytes: installer, outputRoot });
console.log(`created ${file}`);
