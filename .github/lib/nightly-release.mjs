import { readFileSync } from "node:fs";
import { setTimeout as sleep } from "node:timers/promises";
import { githubReleases } from "./github-releases.mjs";
import { macosSigningProvenance, windowsSigningProvenance } from "./release-promotion.mjs";
import { signNightlyUpdateAssets } from "./update-artifacts.mjs";
import { decodeSecretKey } from "./updater-signature.mjs";

// The rolling nightly pre-release lives on GitHub Releases. Forgejo holds the
// source and the nightly tag, and its push mirror carries both to GitHub.
export const NIGHTLY_TAG = "nightly";

const missing = (error) => error?.status === 404 || error?.status === 422;

// The build VMs fetch the source from GitHub, so wait for the push mirror.
export async function waitForMirroredCommit({ github, owner, repo, sha, attempts = 80, wait = () => sleep(15000), log = console.log }) {
  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const { data } = await github.rest.repos.getCommit({ owner, repo, ref: sha });
      if (data.sha === sha) return;
    } catch (error) {
      if (!missing(error)) throw error;
    }
    if (attempt < attempts) {
      log(`waiting for the push mirror to carry ${sha} to ${owner}/${repo}`);
      await wait();
    }
  }
  throw new Error(`${owner}/${repo} does not have ${sha}. Check the Forgejo push mirror.`);
}

// Create the pre-release on the existing nightly tag. A release on a missing
// tag would create that tag on GitHub only, and the next mirror push removes it.
export async function ensureNightlyRelease({ github, owner, repo }) {
  try {
    await github.rest.repos.getReleaseByTag({ owner, repo, tag: NIGHTLY_TAG });
    return;
  } catch (error) {
    if (error.status !== 404) throw error;
  }
  try {
    await github.rest.git.getRef({ owner, repo, ref: `tags/${NIGHTLY_TAG}` });
  } catch (error) {
    if (!missing(error)) throw error;
    throw new Error(`Create the ${NIGHTLY_TAG} tag on Forgejo. The push mirror carries it to ${owner}/${repo}.`);
  }
  await github.rest.repos.createRelease({
    owner, repo,
    tag_name: NIGHTLY_TAG,
    name: "Nightly desktop build",
    body: "Initial nightly build is in progress.",
    prerelease: true,
  });
}

const expectedSuffixes = [
  [".deb"],
  [".AppImage"],
  [".msi", "-machine.msi"],
  ["-machine.msi"],
  ["-nsis.exe"],
  ...["", "-arm64", "-x64"].flatMap(arch =>
    [".app.zip", ".pkg", ".dmg", ".app.tar.gz", ".app.tar.gz.sig"].map(format => [`macos-muniment${arch}${format}`])),
  [".AppImage.sig"],
  [".msi.sig", "-machine.msi.sig"],
  ["-machine.msi.sig"],
  ["-nsis.exe.sig"],
];

// Require one of each asset for the source, remove every other asset, and
// record the source and signing state. The rolling tag stays where it is.
export async function finalizeNightlyRelease({ github, owner, repo, sha, windowsSigning, macosSigning, now = new Date() }) {
  const prefix = `nightly-${sha}-`;
  const { data: release } = await github.rest.repos.getReleaseByTag({ owner, repo, tag: NIGHTLY_TAG });
  for (const [suffix, excludeSuffix] of expectedSuffixes) {
    const matches = release.assets.filter((asset) =>
      asset.name.startsWith(prefix) && asset.name.endsWith(suffix) &&
      (!excludeSuffix || !asset.name.endsWith(excludeSuffix)));
    if (matches.length !== 1) throw new Error(`expected one ${suffix} asset for ${sha}, found ${matches.length}`);
  }
  for (const asset of release.assets) {
    if (!asset.name.startsWith(prefix)) await github.rest.repos.deleteReleaseAsset({ owner, repo, asset_id: asset.id });
  }
  const buildDate = now.toISOString().replace("T", " ").replace(/:\d\d\.\d\d\dZ$/, " UTC");
  const signing = windowsSigning
    ? `\n\n${windowsSigningProvenance(sha)}`
    : "\n\nWindows installers are unsigned; stable promotion is blocked.";
  const macSigning = macosSigning
    ? `\n\n${macosSigningProvenance(sha)}`
    : "\n\nmacOS artifacts are unsigned pending Apple enrollment Y5DUNHQA74.";
  await github.rest.repos.updateRelease({
    owner, repo,
    release_id: release.id,
    name: "Nightly desktop build",
    body: `Automated desktop build from \`${sha}\`.\n\nBuilt ${buildDate}.${signing}${macSigning}\n\nModel weights are not included.`,
    prerelease: true,
  });
}

const usage = "usage: GH_TOKEN=<injected> nightly-release.mjs <ensure|sign|finalize> <owner/repo> <sha> [platform]";

if (import.meta.url === `file://${process.argv[1]}`) {
  const [command, repository, sha, platform] = process.argv.slice(2);
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository ?? "") || !/^[0-9a-f]{40}$/.test(sha ?? "")) throw new Error(usage);
  const [owner, repo] = repository.split("/");
  const github = githubReleases(process.env.GH_TOKEN);
  if (command === "ensure") {
    await waitForMirroredCommit({ github, owner, repo, sha });
    await ensureNightlyRelease({ github, owner, repo });
  } else if (command === "sign") {
    // The updater key stays in this process, which runs only this repository's
    // code and Node's crypto, and each signature is checked before upload.
    const key = decodeSecretKey(process.env.TAURI_SIGNING_PRIVATE_KEY, process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD);
    await signNightlyUpdateAssets({
      github, owner, repo, sha, platform,
      version: JSON.parse(readFileSync("package.json", "utf8")).version,
      key, publicKey: readFileSync("src-tauri/updater.pub", "utf8"),
    });
  } else if (command === "finalize") {
    await finalizeNightlyRelease({
      github, owner, repo, sha,
      windowsSigning: process.env.WINDOWS_SIGNING_ENABLED === "true",
      macosSigning: process.env.MACOS_SIGNING_ENABLED === "true",
    });
  } else {
    throw new Error(usage);
  }
  console.log(`${command} ${repository} nightly for ${sha}`);
}
