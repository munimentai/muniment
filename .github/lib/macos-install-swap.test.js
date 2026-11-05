import { afterEach, expect, it } from 'vitest';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { swapInstalledApp } from './macos-install-swap.mjs';
const directories = [];
afterEach(() => { for (const dir of directories.splice(0)) rmSync(dir, { recursive: true, force: true }); });
function layout() {
  const root = mkdtempSync(join(tmpdir(), 'muniment-swap-test-'));
  directories.push(root);
  const backupRoot = join(root, 'tmp');
  mkdirSync(backupRoot);
  const bundle = (path, text) => { mkdirSync(path, { recursive: true }); writeFileSync(join(path, 'version'), text); return path; };
  return { backupRoot, staged: bundle(join(root, 'stage', 'muniment.app'), 'new'), installed: join(root, 'Applications', 'muniment.app'), bundle };
}
const version = (app) => readFileSync(join(app, 'version'), 'utf8');

it('replaces the installed app and leaves no backup folder', () => {
  const { backupRoot, staged, installed, bundle } = layout();
  bundle(installed, 'old');
  swapInstalledApp(staged, installed, { backupRoot });
  expect(version(installed)).toBe('new');
  expect(readdirSync(backupRoot)).toEqual([]);
});

it('installs a first app and leaves no backup folder', () => {
  const { backupRoot, staged, installed } = layout();
  mkdirSync(join(installed, '..'), { recursive: true });
  swapInstalledApp(staged, installed, { backupRoot });
  expect(version(installed)).toBe('new');
  expect(readdirSync(backupRoot)).toEqual([]);
});

it('restores the old app when the new one cannot move and removes the backup folder', () => {
  const { backupRoot, staged, installed, bundle } = layout();
  bundle(installed, 'old');
  const rename = (from, to) => { if (from === staged) throw new Error('EXDEV'); renameSync(from, to); };
  expect(() => swapInstalledApp(staged, installed, { backupRoot, rename })).toThrow('EXDEV');
  expect(version(installed)).toBe('old');
  expect(readdirSync(backupRoot)).toEqual([]);
});

it('keeps the backup and names its path when the restore also fails', () => {
  const { backupRoot, staged, installed, bundle } = layout();
  bundle(installed, 'old');
  let moves = 0;
  const rename = (from, to) => { if (++moves > 1) throw new Error(`move ${moves} refused`); renameSync(from, to); };
  let failure;
  try { swapInstalledApp(staged, installed, { backupRoot, rename }); } catch (error) { failure = error; }
  const [backup] = readdirSync(backupRoot);
  expect(backup).toMatch(/^muniment-app-backup-/);
  expect(failure.message).toContain(`The old app is in ${join(backupRoot, backup)}.`);
  expect(version(join(backupRoot, backup, 'muniment.app'))).toBe('old');
  expect(existsSync(installed)).toBe(false);
});
