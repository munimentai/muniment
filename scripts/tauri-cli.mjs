import { ensureCli } from '../prototypes/cef-browser/scripts/tauri-cli.mjs';
import { spawnSync } from 'node:child_process';
const result = spawnSync(ensureCli(), ['tauri', ...process.argv.slice(2)], { stdio: 'inherit' });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
