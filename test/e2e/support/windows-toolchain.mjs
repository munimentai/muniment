import fs from 'node:fs'
import path from 'node:path'
import { spawnSync } from 'node:child_process'
import { redactText } from './redact-text.mjs'

const [root, phase] = process.argv.slice(2)
const bin = path.join(root, 'node_modules', '.bin')
const shim = path.join(bin, 'tauri.cmd')
const cli = path.join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js')
const normalize = (value) => path.resolve(value.replace(/^"|"$/g, '')).toLowerCase()
const paths = Object.keys(process.env).filter((key) => /^path$/i.test(key))
  .flatMap((key) => process.env[key].split(path.delimiter)).filter(Boolean)
const hasBin = paths.some((value) => normalize(value) === normalize(bin))
const hasCmd = (process.env.PATHEXT || '').split(';').some((value) => value.toUpperCase() === '.CMD')
const where = process.env.SystemRoot ? path.join(process.env.SystemRoot, 'System32', 'where.exe') : 'where.exe'
const found = spawnSync(where, ['tauri'], { encoding: 'utf8' })
const hasCli = fs.statSync(cli, { throwIfNoEntry: false })?.isFile() === true
const hasShim = fs.statSync(shim, { throwIfNoEntry: false })?.isFile() === true
const reasons = []
if (!hasCli) reasons.push('The local Tauri CLI entry is missing.')
if (!hasShim) reasons.push('The local tauri.cmd shim is missing.')
if (!hasBin) reasons.push('The npm command PATH omits the local node_modules\\.bin directory.')
if (!hasCmd) reasons.push('PATHEXT omits .CMD, so cmd.exe cannot resolve the tauri.cmd shim as tauri.')
if (found.error) reasons.push(`The toolchain probe could not start where.exe: ${found.error.message}`)
else if (found.status !== 0) reasons.push('where.exe cannot resolve tauri.')

// Print only command resolution fields, not the full npm environment.
console.log(redactText([
  `Tauri toolchain ${phase}`,
  `cwd=${process.cwd()}`,
  `npm_local_prefix=${process.env.npm_config_local_prefix || ''}`,
  `npm_execpath=${process.env.npm_execpath || ''}`,
  `node=${process.execPath}`,
  `cli=${cli} present=${hasCli}`,
  `shim=${shim} present=${hasShim}`,
  `npm_path_has_local_bin=${hasBin} pathext_has_cmd=${hasCmd}`,
  `where_tauri=${found.stdout?.trim() || 'unavailable'}`,
  ...reasons,
].join('\n')))
