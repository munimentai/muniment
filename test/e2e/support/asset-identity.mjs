import fs from 'node:fs'

const [sha, selector = '-', selectedFile = '-'] = process.argv.slice(2)
const platform = ['linux', 'linux-appimage', 'windows', 'macos'].includes(selector) ? selector : 'linux'
const file = platform === selector ? selectedFile : selector
if (!/^[0-9a-f]{40}$/.test(sha || '')) throw new Error('invalid source SHA')
const release = JSON.parse(file === '-' ? fs.readFileSync(0, 'utf8') : fs.readFileSync(file, 'utf8'))
const windowsPerUser = new RegExp(`^nightly-${sha}-windows-muniment_[0-9]+\\.[0-9]+\\.[0-9]+_x64_en-US\\.msi$`)
const linuxAppImage = new RegExp(`^nightly-${sha}-linux-muniment_[0-9]+\\.[0-9]+\\.[0-9]+_amd64\\.AppImage$`)
const expectedName = platform === 'macos'
  ? `nightly-${sha}-macos-muniment.app.zip`
  : `nightly-${sha}-linux-muniment.deb`
const matches = release.assets.filter((asset) => platform === 'windows'
  ? windowsPerUser.test(asset.name)
  : platform === 'linux-appimage' ? linuxAppImage.test(asset.name) : asset.name === expectedName)
if (matches.length !== 1) {
  const expected = platform === 'windows' ? windowsPerUser.source : platform === 'linux-appimage' ? linuxAppImage.source : expectedName
  throw new Error(`missing or duplicate ${platform} artifact: expected ${expected}, matches=${matches.length}, release assets=${JSON.stringify(release.assets.map((asset) => asset.name))}`)
}
if (!Number.isSafeInteger(matches[0].id) || matches[0].id <= 0) throw new Error('invalid artifact identity')
process.stdout.write(String(matches[0].id))
