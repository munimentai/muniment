import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')

const missingPackageNotices = (packageJson, packageLock, notices) => {
  const bundledPackages = [...Object.keys(packageJson.dependencies), 'svelte']

  return bundledPackages.filter((name) => {
    const version = packageLock.packages[`node_modules/${name}`]?.version
    return !version || !notices.includes(`\`${name}\` ${version} —`)
  })
}

describe('third-party notices', () => {
  it('records every bundled package at its resolved version', () => {
    const packageJson = JSON.parse(read('package.json'))
    const packageLock = JSON.parse(read('package-lock.json'))
    const notices = read('THIRD_PARTY_NOTICES.md')

    expect(missingPackageNotices(packageJson, packageLock, notices)).toEqual([])
  })

  it('rejects an unnamed package', () => {
    const packageJson = { dependencies: { unnamed: '1.0.0' } }
    const packageLock = { packages: {
      'node_modules/unnamed': { version: '1.0.0' },
      'node_modules/svelte': { version: '5.56.4' },
    } }
    const notices = '`svelte` 5.56.4 — MIT'

    expect(missingPackageNotices(packageJson, packageLock, notices)).toEqual(['unnamed'])
  })

  it('rejects a package recorded at a different version', () => {
    const packageJson = { dependencies: {} }
    const packageLock = { packages: { 'node_modules/svelte': { version: '5.56.4' } } }

    expect(missingPackageNotices(packageJson, packageLock, '`svelte` 5.56.3 — MIT')).toEqual(['svelte'])
  })
})
