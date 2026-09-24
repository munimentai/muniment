import { readdir } from 'node:fs/promises';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { signUpdaterBytes, verifyUpdaterSignature } from './updater-signature.mjs';

export const artifactSpecs = {
  linux: [['deb', '.deb'], ['appimage', '.AppImage']],
  windows: [['msi', '.msi', '-machine.msi'], ['msi', '-machine.msi'], ['nsis', '-setup.exe']],
  macos: [['macos', '.app.zip'], ['pkg', '.pkg'], ['macos', '.app.tar.gz']],
};
export const isUpdateArtifact = (name) => name.endsWith('.AppImage') || name.endsWith('.app.tar.gz') ||
  name.endsWith('.msi') || name.endsWith('-setup.exe') || name.endsWith('-nsis.exe');

// Collect the bundles a platform build uploads. The build VM holds no updater
// key: the nightly signs each update bundle on the runner after the upload, so
// this step only archives the macOS app and names the files.
export async function prepareUpdateArtifacts(platform, base, run = spawnSync) {
  const execute = (command, args) => {
    const result = run(command, args, { stdio: 'pipe', encoding: 'utf8', env: { ...process.env, COPYFILE_DISABLE: '1' } });
    if (result.error || result.status !== 0) throw new Error(`Update artifact command failed: ${command}`);
  };
  if (platform === 'macos') {
    const directory = join(base, 'macos');
    const apps = (await readdir(directory)).filter((name) => name.endsWith('.app'));
    if (apps.length !== 1) throw new Error('Expected one signed macOS application');
    execute('tar', ['-czf', join(directory, `${apps[0]}.tar.gz`), '-C', directory, apps[0]]);
  }
  const files = [];
  for (const [directory, suffix, exclude] of artifactSpecs[platform] ?? []) {
    const matches = (await readdir(join(base, directory))).filter((name) => name.endsWith(suffix) && (!exclude || !name.endsWith(exclude)));
    if (matches.length !== 1) throw new Error(`Expected one ${suffix} artifact`);
    files.push(join(base, directory, matches[0]));
  }
  if (!files.length) throw new Error(`Unsupported update platform: ${platform}`);
  return files;
}

const updateBundleCount = { linux: 1, windows: 3, macos: 1 };

// Sign one platform's update bundles on the nightly release. This runs on the
// runner in a step that holds the updater key and nothing else, after the build
// VM uploads the bundles, and it checks each signature against the committed
// public key before the upload.
export async function signNightlyUpdateAssets({ github, owner, repo, sha, platform, version, key, publicKey, log = console.log }) {
  if (!/^[0-9a-f]{40}$/.test(sha) || !updateBundleCount[platform]) throw new Error('A source SHA and a known platform are required');
  const prefix = `nightly-${sha}-${platform}-`;
  const { data: release } = await github.rest.repos.getReleaseByTag({ owner, repo, tag: 'nightly' });
  const assets = release.assets.filter(({ name }) => name.startsWith(prefix));
  const bundles = assets.filter(({ name }) => isUpdateArtifact(name));
  if (bundles.length !== updateBundleCount[platform]) {
    throw new Error(`Expected ${updateBundleCount[platform]} ${platform} update bundles for ${sha}, found ${bundles.length}`);
  }
  for (const asset of bundles) {
    const { data } = await github.rest.repos.getReleaseAsset({ owner, repo, asset_id: asset.id, headers: { accept: 'application/octet-stream' } });
    const bytes = Buffer.from(data);
    const digest = `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
    if (asset.digest && asset.digest !== digest) throw new Error(`Downloaded ${asset.name} does not match its release digest`);
    const signature = signUpdaterBytes(bytes, key, { fileName: asset.name.slice(prefix.length), version });
    verifyUpdaterSignature(bytes, signature, publicKey);
    const name = `${asset.name}.sig`;
    const old = release.assets.find((candidate) => candidate.name === name);
    if (old) await github.rest.repos.deleteReleaseAsset({ owner, repo, asset_id: old.id });
    await github.rest.repos.uploadReleaseAsset({
      owner, repo, release_id: release.id, name, data: signature,
      headers: { 'content-type': 'application/octet-stream' },
    });
    log(`signed ${asset.name}`);
  }
}
