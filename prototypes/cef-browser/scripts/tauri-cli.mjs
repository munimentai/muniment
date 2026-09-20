import { existsSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

// Pin the CLI to the same upstream source as the Rust application dependencies.
export const revision = '747a0612d5dba9829fb3487a3ed6cdf7b5e81dd9';
const root = fileURLToPath(new URL('..', import.meta.url));
const source = join(root, '.tools', 'tauri', revision);
function run(command, args, cwd = root) {
  const result = spawnSync(command, args, { cwd, stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command} failed with status ${result.status}`);
}
export function ensureCli() {
  if (!existsSync(join(source, '.git'))) {
    mkdirSync(source, { recursive: true });
    run('git', ['init', source]);
  }
  const head = spawnSync('git', ['rev-parse', 'HEAD'], { cwd: source, encoding: 'utf8' });
  if (head.status !== 0 || head.stdout.trim() !== revision) {
    run('git', ['fetch', '--depth', '1', 'git@github.com:tauri-apps/tauri.git', revision], source);
    run('git', ['checkout', '--detach', 'FETCH_HEAD'], source);
  }
  const binary = join(source, 'target', 'release', 'cargo-tauri');
  if (!existsSync(binary)) {
    run('cargo', ['build', '--release', '--locked', '--manifest-path', 'crates/tauri-cli/Cargo.toml'], source);
  }
  return binary;
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  run(ensureCli(), ['tauri', ...process.argv.slice(2)]);
}
