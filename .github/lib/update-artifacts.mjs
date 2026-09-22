import { readdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

export const artifactSpecs = {
  linux: [['deb', '.deb'], ['appimage', '.AppImage']],
  windows: [['msi', '.msi', '-machine.msi'], ['msi', '-machine.msi'], ['nsis', '-setup.exe']],
  macos: [['macos', '.app.zip'], ['pkg', '.pkg'], ['macos', '.app.tar.gz']],
};
export const isUpdateArtifact = (name) => name.endsWith('.AppImage') || name.endsWith('.app.tar.gz') ||
  name.endsWith('.msi') || name.endsWith('-setup.exe') || name.endsWith('-nsis.exe');

export async function prepareUpdateArtifacts(platform, base, run = spawnSync) {
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY || !process.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    throw new Error('Updater signing credentials are required');
  }
  const execute = (command, args) => {
    const result = run(command, args, { stdio: 'pipe', encoding: 'utf8', env: { ...process.env, COPYFILE_DISABLE: '1' } });
    // Signing output stays private. No key material or process environment is logged.
    if (result.error || result.status !== 0) throw new Error(`Update artifact command failed: ${command}`);
  };
  if (platform === 'macos') {
    const directory = join(base, 'macos');
    const apps = (await readdir(directory)).filter((name) => name.endsWith('.app'));
    if (apps.length !== 1) throw new Error('Expected one signed macOS application');
    execute('tar', ['-czf', join(directory, `${apps[0]}.tar.gz`), '-C', directory, apps[0]]);
  }
  const version = JSON.parse(await readFile('package.json', 'utf8')).version;
  const files = [];
  for (const [directory, suffix, exclude] of artifactSpecs[platform] ?? []) {
    const matches = (await readdir(join(base, directory))).filter((name) => name.endsWith(suffix) && (!exclude || !name.endsWith(exclude)));
    if (matches.length !== 1) throw new Error(`Expected one ${suffix} artifact`);
    const file = join(base, directory, matches[0]);
    files.push(file);
    if (isUpdateArtifact(file)) {
      execute(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js', 'signer', 'sign', '--app-version', version, file]);
      await readFile(`${file}.sig`); // Missing signatures stop upload.
      files.push(`${file}.sig`);
    }
  }
  if (!files.length) throw new Error(`Unsupported update platform: ${platform}`);
  return files;
}
