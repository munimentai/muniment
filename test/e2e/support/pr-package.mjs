import fs from 'node:fs'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

// Local package names the unsigned build helpers write. The per-user MSI name
// excludes the machine MSI that the same build writes beside it.
const PACKAGES = {
  'linux-deb': /^[^/\\]+\.deb$/,
  windows: /^muniment_[0-9]+\.[0-9]+\.[0-9]+_x64_en-US\.msi$/,
  macos: /^muniment\.app\.zip$/,
}

export function selectLocalPackage(platform, directory, list = (dir) => fs.readdirSync(dir)) {
  const pattern = PACKAGES[platform]
  if (!pattern) throw new Error('unknown package platform')
  let names
  try {
    names = list(directory)
  } catch {
    throw new Error(`the ${platform} package directory is unavailable: ${directory}`)
  }
  const matches = names.filter((name) => pattern.test(name))
  if (matches.length !== 1) {
    throw new Error(`missing or duplicate local ${platform} package: matches=${matches.length}, directory entries=${JSON.stringify(names)}`)
  }
  return path.resolve(directory, matches[0])
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [command, platform, directory] = process.argv.slice(2)
  try {
    if (command !== 'select' || !platform || !directory) throw new Error('usage: pr-package.mjs select <linux-deb|windows|macos> <directory>')
    process.stdout.write(selectLocalPackage(platform, directory))
  } catch (error) {
    console.error(error.message)
    process.exit(1)
  }
}
