import { afterEach, describe, expect, it, vi } from 'vitest';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { isUpdateArtifact, prepareUpdateArtifacts } from './update-artifacts.mjs';
afterEach(() => vi.unstubAllEnvs());
describe('update artifact preparation', () => {
  it('requires signing secrets before preparing an upload', async () => {
    vi.stubEnv('TAURI_SIGNING_PRIVATE_KEY', '');
    await expect(prepareUpdateArtifacts('linux', '/missing')).rejects.toThrow('credentials');
  });
  it('signs both MSI scopes and NSIS but not unrelated packages', () => {
    for (const name of ['app.msi', 'app-machine.msi', 'app-setup.exe', 'app.AppImage', 'app.app.tar.gz']) expect(isUpdateArtifact(name)).toBe(true);
    for (const name of ['app.pkg', 'app.deb', 'app.app.zip', 'app.msi.sig']) expect(isUpdateArtifact(name)).toBe(false);
  });
  it.skipIf(process.platform === 'win32')('archives the signed app with links and binds its signature to the app version', async () => {
    vi.stubEnv('TAURI_SIGNING_PRIVATE_KEY', 'fixture');
    vi.stubEnv('TAURI_SIGNING_PRIVATE_KEY_PASSWORD', 'fixture');
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
        if (command === 'tar') return spawnSync(command, args, options);
        fs.writeFileSync(`${args.at(-1)}.sig`, 'signed fixture');
        return { status: 0 };
      });
      expect(files).toHaveLength(4);
      const archive = path.join(macos, 'muniment.app.tar.gz');
      const unpacked = path.join(root, 'unpacked'); fs.mkdirSync(unpacked);
      expect(spawnSync('tar', ['-xzf', archive, '-C', unpacked]).status).toBe(0);
      const framework = path.join(unpacked, 'muniment.app/Contents/Frameworks');
      expect(fs.lstatSync(path.join(framework, 'current')).isSymbolicLink()).toBe(true);
      expect(fs.readFileSync(path.join(framework, 'current'), 'utf8')).toBe('signed fixture bytes');
      expect(calls[1][1]).toEqual(['node_modules/@tauri-apps/cli/tauri.js', 'signer', 'sign', '--app-version', JSON.parse(fs.readFileSync('package.json')).version, archive]);
    } finally { fs.rmSync(root, { recursive: true, force: true }); }
  });
});
