import { afterEach, describe, expect, it, vi } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createHash, generateKeyPairSync } from 'node:crypto';
import { isUpdateArtifact, prepareUpdateArtifacts, signNightlyUpdateAssets } from './update-artifacts.mjs';
import { verifyUpdaterSignature } from './updater-signature.mjs';
afterEach(() => vi.unstubAllEnvs());
describe('update artifact preparation', () => {
  it('names the bundles without reading an updater key', async () => {
    vi.stubEnv('TAURI_SIGNING_PRIVATE_KEY', '');
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-update-artifacts-'));
    try {
      for (const [directory, name] of [['deb', 'muniment.deb'], ['appimage', 'muniment.AppImage']]) {
        fs.mkdirSync(path.join(root, directory));
        fs.writeFileSync(path.join(root, directory, name), name);
      }
      const run = vi.fn();
      expect(await prepareUpdateArtifacts('linux', root, run)).toEqual([path.join(root, 'deb/muniment.deb'), path.join(root, 'appimage/muniment.AppImage')]);
      expect(run).not.toHaveBeenCalled();
    } finally { fs.rmSync(root, { recursive: true, force: true }); }
  });
  it('signs both MSI scopes and NSIS but not unrelated packages', () => {
    for (const name of ['app.msi', 'app-machine.msi', 'app-setup.exe', 'app.AppImage', 'app.app.tar.gz']) expect(isUpdateArtifact(name)).toBe(true);
    for (const name of ['app.pkg', 'app.deb', 'app.app.zip', 'app.msi.sig']) expect(isUpdateArtifact(name)).toBe(false);
  });
  it.skipIf(process.platform === 'win32')('archives the signed app with links and signs nothing in the build VM', async () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'muniment-update-artifacts-'));
    const macos = path.join(root, 'macos');
    fs.mkdirSync(path.join(macos, 'muniment.app/Contents/Frameworks'), { recursive: true });
    fs.writeFileSync(path.join(macos, 'muniment.app/Contents/Frameworks/library'), 'signed fixture bytes');
    fs.symlinkSync('library', path.join(macos, 'muniment.app/Contents/Frameworks/current'));
    fs.writeFileSync(path.join(macos, 'muniment.app.zip'), 'zip');
    fs.mkdirSync(path.join(root, 'pkg'));
    fs.writeFileSync(path.join(root, 'pkg/muniment.pkg'), 'pkg');
    const calls = [];
    try {
      const files = await prepareUpdateArtifacts('macos', root, (command, args, options) => {
        calls.push([command, args]);
        return spawnSync(command, args, options);
      });
      expect(files).toHaveLength(3);
      expect(files.some((file) => file.endsWith('.sig'))).toBe(false);
      const archive = path.join(macos, 'muniment.app.tar.gz');
      const unpacked = path.join(root, 'unpacked'); fs.mkdirSync(unpacked);
      expect(spawnSync('tar', ['-xzf', archive, '-C', unpacked]).status).toBe(0);
      const framework = path.join(unpacked, 'muniment.app/Contents/Frameworks');
      expect(fs.lstatSync(path.join(framework, 'current')).isSymbolicLink()).toBe(true);
      expect(fs.readFileSync(path.join(framework, 'current'), 'utf8')).toBe('signed fixture bytes');
      expect(calls.map(([command]) => command)).toEqual(['tar']);
    } finally { fs.rmSync(root, { recursive: true, force: true }); }
  });
});

describe('nightly update signing', () => {
  const sha = 'a'.repeat(40);
  const { privateKey, publicKey } = generateKeyPairSync('ed25519');
  const keyId = Buffer.from('0102030405060708', 'hex');
  const pk = publicKey.export({ format: 'der', type: 'spki' }).subarray(-32);
  const publicText = Buffer.from(`untrusted comment: minisign public key\n${Buffer.concat([Buffer.from('Ed'), keyId, pk]).toString('base64')}\n`).toString('base64');
  const bundle = Buffer.from('msi bytes');
  const digest = `sha256:${createHash('sha256').update(bundle).digest('hex')}`;
  const fakeGithub = (assets) => {
    const uploads = [];
    const deleted = [];
    return {
      uploads, deleted,
      rest: { repos: {
        getReleaseByTag: async () => ({ data: { id: 7, assets } }),
        getReleaseAsset: async () => ({ data: bundle.buffer.slice(bundle.byteOffset, bundle.byteOffset + bundle.length) }),
        deleteReleaseAsset: async ({ asset_id }) => { deleted.push(asset_id); },
        uploadReleaseAsset: async (request) => { uploads.push(request); },
      } },
    };
  };
  const windows = [
    { id: 1, name: `nightly-${sha}-windows-muniment_0.0.1_x64_en-US.msi`, digest },
    { id: 2, name: `nightly-${sha}-windows-muniment_0.0.1_x64_en-US-machine.msi`, digest },
    { id: 3, name: `nightly-${sha}-windows-muniment_0.0.1_x64-nsis.exe`, digest },
    { id: 4, name: `nightly-${sha}-windows-muniment_0.0.1_x64-nsis.exe.sig` },
    { id: 5, name: `nightly-${'b'.repeat(40)}-windows-muniment_0.0.1_x64-nsis.exe`, digest },
  ];

  it('signs each bundle, verifies it, and replaces an older signature', async () => {
    const github = fakeGithub(windows);
    await signNightlyUpdateAssets({ github, owner: 'o', repo: 'r', sha, platform: 'windows', version: '0.0.1', key: { keyId, privateKey }, publicKey: publicText, log: () => {} });
    expect(github.uploads.map(({ name }) => name)).toEqual([1, 2, 3].map((id) => `${windows[id - 1].name}.sig`));
    expect(github.deleted).toEqual([4]);
    for (const { data, release_id } of github.uploads) {
      expect(release_id).toBe(7);
      expect(verifyUpdaterSignature(bundle, data, publicText)).toMatch(/\tversion:0\.0\.1$/);
    }
  });

  it('stops on a missing bundle or a digest mismatch', async () => {
    await expect(signNightlyUpdateAssets({ github: fakeGithub(windows.slice(1)), owner: 'o', repo: 'r', sha, platform: 'windows', version: '0.0.1', key: { keyId, privateKey }, publicKey: publicText }))
      .rejects.toThrow('Expected 3 windows update bundles');
    const github = fakeGithub([{ id: 9, name: `nightly-${sha}-linux-muniment.AppImage`, digest: `sha256:${'0'.repeat(64)}` }]);
    await expect(signNightlyUpdateAssets({ github, owner: 'o', repo: 'r', sha, platform: 'linux', version: '0.0.1', key: { keyId, privateKey }, publicKey: publicText }))
      .rejects.toThrow('does not match its release digest');
    expect(github.uploads).toHaveLength(0);
  });
});
