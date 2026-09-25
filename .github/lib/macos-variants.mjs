import { closeSync, existsSync, mkdirSync, mkdtempSync, openSync, readdirSync, readSync, renameSync, rmSync, symlinkSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';

export const macosArchitectures = ['', '-arm64', '-x64'];
export const macosFormats = ['.app.zip', '.app.tar.gz', '.pkg', '.dmg'];
const magic = new Set(['cffaedfe', 'feedfacf', 'cefaedfe', 'feedface', 'cafebabe', 'bebafeca', 'cafebabf', 'bfbafeca']);
const execute = (command, args) => {
  const result = spawnSync(command, args, { encoding: 'utf8', maxBuffer: 1024 * 1024 });
  if (result.error || result.status !== 0) throw new Error(`${command} failed: ${result.stderr ?? result.error}`);
  return result.stdout;
};
export function macosVariants(base) {
  return macosArchitectures.map(suffix => ({
    suffix, arch: suffix === '-arm64' ? 'arm64' : suffix === '-x64' ? 'x86_64' : null,
    app: join(base, 'macos', ...(suffix ? [suffix.slice(1)] : []), 'muniment.app'),
    zip: join(base, 'macos', `muniment${suffix}.app.zip`),
    tar: join(base, 'macos', `muniment${suffix}.app.tar.gz`),
    pkg: join(base, 'pkg', `muniment${suffix}.pkg`),
    dmg: join(base, 'dmg', `muniment${suffix}.dmg`),
  }));
}
export function nativeFiles(directory) {
  return readdirSync(directory, { recursive: true, withFileTypes: true })
    .filter(entry => entry.isFile())
    .map(entry => join(entry.parentPath ?? entry.path, entry.name))
    .filter(file => {
      const fd = openSync(file, 'r'); const header = Buffer.alloc(4);
      try { readSync(fd, header, 0, 4, 0); } finally { closeSync(fd); }
      return magic.has(header.toString('hex'));
    });
}
// Derive thin apps before signing. CEF, speech libraries and helper processes
// must all match the selected architecture, not just the main executable.
export function prepareMacosVariants(base) {
  const variants = macosVariants(base);
  for (const variant of variants.slice(1)) {
    if (existsSync(variant.app)) throw new Error(`Variant already exists: ${variant.app}`);
    mkdirSync(dirname(variant.app), { recursive: true });
    execute('ditto', [variants[0].app, variant.app]);
    const files = nativeFiles(variant.app);
    if (!files.length) throw new Error('Application has no native code');
    for (const file of files) {
      const arches = execute('lipo', ['-archs', file]).trim().split(/\s+/);
      if (!arches.includes(variant.arch)) throw new Error(`${file} lacks ${variant.arch}`);
      if (arches.length > 1) {
        execute('lipo', [file, '-thin', variant.arch, '-output', `${file}.thin`]);
        renameSync(`${file}.thin`, file);
      }
      if (execute('lipo', ['-archs', file]).trim() !== variant.arch) throw new Error(`Wrong architecture: ${file}`);
    }
    const resources = join(variant.app, 'Contents/Frameworks/Chromium Embedded Framework.framework/Resources');
    if (existsSync(resources)) for (const name of readdirSync(resources)) {
      if (name === `v8_context_snapshot.${variant.arch === 'arm64' ? 'x86_64' : 'arm64'}.bin`) rmSync(join(resources, name));
    }
  }
  return variants;
}
export function packageMacosDmg(app, output, run = execute) {
  const temporary = mkdtempSync(join(tmpdir(), 'muniment-dmg-'));
  try {
    const contents = join(temporary, 'contents'); mkdirSync(contents);
    run('ditto', [app, join(contents, 'muniment.app')]);
    symlinkSync('/Applications', join(contents, 'Applications'));
    mkdirSync(dirname(output), { recursive: true });
    run('hdiutil', ['create', '-volname', 'Muniment', '-srcfolder', contents, '-format', 'UDZO', '-fs', 'HFS+', '-ov', output]);
    run('hdiutil', ['verify', output]);
  } finally { rmSync(temporary, { recursive: true, force: true }); }
}
