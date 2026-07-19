import fs from 'node:fs'

const [sha, selector = '-', selectedFile = '-'] = process.argv.slice(2)
const platform = ['linux', 'windows'].includes(selector) ? selector : 'linux'
const file = platform === selector ? selectedFile : selector
if (!/^[0-9a-f]{40}$/.test(sha || '')) throw new Error('invalid source SHA')
const release = JSON.parse(file === '-' ? fs.readFileSync(0, 'utf8') : fs.readFileSync(file, 'utf8'))
if (release.target_commitish !== sha) throw new Error('release identity mismatch')
const windowsPerUser = new RegExp(`^nightly-${sha}-windows-muniment_[0-9]+\\.[0-9]+\\.[0-9]+_x64_en-US\\.msi$`)
const matches = release.assets.filter((asset) => platform === 'windows'
  ? windowsPerUser.test(asset.name)
  : asset.name === `nightly-${sha}-linux-muniment.deb`)
if (matches.length !== 1) throw new Error(`missing or duplicate ${platform} artifact`)
if (!Number.isSafeInteger(matches[0].id) || matches[0].id <= 0) throw new Error('invalid artifact identity')
process.stdout.write(String(matches[0].id))
