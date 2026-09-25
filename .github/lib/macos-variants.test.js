import { afterEach, describe, expect, it } from 'vitest';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { macosVariants, nativeFiles, packageMacosDmg, prepareMacosVariants } from './macos-variants.mjs';
const directories = [];
afterEach(() => { for (const dir of directories.splice(0)) rmSync(dir, { recursive: true, force: true }); });
const temporary = () => { const dir = mkdtempSync(join(tmpdir(), 'muniment-variant-test-')); directories.push(dir); return dir; };
const run = (cmd, args) => execFileSync(cmd, args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();

it('gives each architecture its own downloads and keeps the app bundle name', () => {
  const variants = macosVariants('/bundle');
  expect(new Set(variants.flatMap(v => [v.zip, v.tar, v.pkg, v.dmg])).size).toBe(12);
  expect(variants.map(v => v.arch)).toEqual([null, 'arm64', 'x86_64']);
  expect(variants.every(v => v.app.endsWith('/muniment.app'))).toBe(true);
});

describe.skipIf(process.platform !== 'darwin')('native Mac packaging', () => {
  it('thins nested native code, preserves resources, and mounts a drag-install DMG', () => {
    const root = temporary(); const [universal] = macosVariants(root);
    const source = join(root, 'fixture.c'); writeFileSync(source, 'int main(void) { return 0; }\n');
    const binary = join(universal.app, 'Contents/MacOS/fixture'); mkdirSync(dirname(binary), { recursive: true });
    run('clang', ['-arch', 'arm64', '-arch', 'x86_64', source, '-o', binary]);
    const nested = join(universal.app, 'Contents/Frameworks/helper'); mkdirSync(dirname(nested), { recursive: true }); copyFileSync(binary, nested);
    writeFileSync(join(universal.app, 'Contents/Info.plist'), '<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>fixture</string><key>CFBundleIdentifier</key><string>ai.muniment.packaging-test</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>');
    writeFileSync(join(universal.app, 'Contents/resource.txt'), 'preserved');
    const variants = prepareMacosVariants(root);
    expect(run('lipo', ['-archs', binary]).split(/\s+/).sort()).toEqual(['arm64', 'x86_64']);
    for (const variant of variants.slice(1)) {
      expect(nativeFiles(variant.app)).toHaveLength(2);
      for (const file of nativeFiles(variant.app)) expect(run('lipo', ['-archs', file])).toBe(variant.arch);
      expect(readFileSync(join(variant.app, 'Contents/resource.txt'), 'utf8')).toBe('preserved');
    }
    const arm = variants[1]; packageMacosDmg(arm.app, arm.dmg);
    const mount = join(root, 'mounted'); mkdirSync(mount);
    run('hdiutil', ['attach', arm.dmg, '-readonly', '-nobrowse', '-mountpoint', mount]);
    try {
      expect(readlinkSync(join(mount, 'Applications'))).toBe('/Applications');
      expect(run('lipo', ['-archs', join(mount, 'muniment.app/Contents/MacOS/fixture')])).toBe('arm64');
    } finally { run('hdiutil', ['detach', mount]); }
    expect(() => prepareMacosVariants(root)).toThrow('already exists');
  }, 120_000);
  it('rejects a helper that lacks the requested architecture', () => {
    const root = temporary(); const [universal] = macosVariants(root);
    const binary = join(universal.app, 'Contents/MacOS/fixture'); mkdirSync(dirname(binary), { recursive: true });
    const source = join(root, 'fixture.c'); writeFileSync(source, 'int main(void) { return 0; }\n');
    run('clang', ['-arch', 'x86_64', source, '-o', binary]);
    expect(() => prepareMacosVariants(root)).toThrow('lacks arm64');
  });
});
