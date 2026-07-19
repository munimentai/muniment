import fs from 'node:fs'

const [sha, file = '-'] = process.argv.slice(2)
if (!/^[0-9a-f]{40}$/.test(sha || '')) throw new Error('invalid source SHA')
const release = JSON.parse(file === '-' ? fs.readFileSync(0, 'utf8') : fs.readFileSync(file, 'utf8'))
if (release.target_commitish !== sha) throw new Error('release identity mismatch')
const name = `nightly-${sha}-linux-muniment.deb`
const matches = release.assets.filter((asset) => asset.name === name)
if (matches.length !== 1) throw new Error('missing or duplicate Linux artifact')
if (!Number.isSafeInteger(matches[0].id) || matches[0].id <= 0) throw new Error('invalid artifact identity')
process.stdout.write(String(matches[0].id))
