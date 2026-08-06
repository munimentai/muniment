import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')
const bundleConfigs = [
  'src-tauri/tauri.linux.conf.json',
  'src-tauri/tauri.macos.conf.json',
  'src-tauri/tauri.windows.conf.json',
]
const requiredResources = {
  '../THIRD_PARTY_NOTICES.md': 'third-party-notices/THIRD_PARTY_NOTICES.md',
  '../THIRD_PARTY_RUST_NOTICES.md': 'third-party-notices/THIRD_PARTY_RUST_NOTICES.md',
  '../src/fonts/SchibstedGrotesk-OFL.txt': 'third-party-notices/SchibstedGrotesk-OFL.txt',
  '../src/fonts/CommitMono-LICENSE.txt': 'third-party-notices/CommitMono-LICENSE.txt',
}

const cargoPackages = (lockfile) => [...lockfile.matchAll(
  // Nightly 31109203906 confirmed that Git checks out the lockfile with CRLF on Windows.
  /\[\[package\]\]\r?\nname = "([^"]+)"\r?\nversion = "([^"]+)"\r?\nsource = /g,
)].map(([, name, version]) => `${name}\t${version}`).sort()

const recordedCargoPackages = (notices) => [...notices.matchAll(
  /^- `([^`]+)` ([^ ]+) — .+$/gm,
)].map(([, name, version]) => `${name}\t${version}`).sort()

const missingPackageNotices = (packageJson, packageLock, notices) => {
  const bundledPackages = [...Object.keys(packageJson.dependencies), 'svelte']

  return bundledPackages.filter((name) => {
    const version = packageLock.packages[`node_modules/${name}`]?.version
    return !version || !notices.includes(`\`${name}\` ${version} —`)
  })
}

const missingBundleResources = (config, exists = fs.existsSync) => {
  const resources = config.bundle.resources

  return Object.entries(requiredResources).filter(([source, destination]) => {
    const sourcePath = path.resolve(root, 'src-tauri', source)
    return resources[source] !== destination || !exists(sourcePath)
  }).map(([source]) => source)
}

describe('third-party notices', () => {
  it('records every bundled package at its resolved version', () => {
    const packageJson = JSON.parse(read('package.json'))
    const packageLock = JSON.parse(read('package-lock.json'))
    const notices = read('THIRD_PARTY_NOTICES.md')

    expect(missingPackageNotices(packageJson, packageLock, notices)).toEqual([])
  })

  it('records every non-workspace crate at its locked version', () => {
    const lockfile = read('src-tauri/Cargo.lock')
    const notices = read('THIRD_PARTY_RUST_NOTICES.md')

    expect(recordedCargoPackages(notices)).toEqual(cargoPackages(lockfile))
  })

  it('rejects a crate recorded at a different version', () => {
    const lockfile = '[[package]]\nname = "example"\nversion = "1.0.0"\nsource = "registry"'
    const notices = '- `example` 1.0.1 — MIT'

    expect(recordedCargoPackages(notices)).not.toEqual(cargoPackages(lockfile))
  })

  it('rejects a crate recorded under a different name', () => {
    const lockfile = '[[package]]\nname = "example"\nversion = "1.0.0"\nsource = "registry"'
    const notices = '- `other` 1.0.0 — MIT'

    expect(recordedCargoPackages(notices)).not.toEqual(cargoPackages(lockfile))
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

  it('ships the notices record and font licenses in every desktop bundle', () => {
    const notices = read('THIRD_PARTY_NOTICES.md')

    for (const file of bundleConfigs) {
      const config = JSON.parse(read(file))

      expect(missingBundleResources(config), file).toEqual([])
    }

    for (const destination of Object.values(requiredResources).slice(1)) {
      expect(notices).toContain(`\`${destination}\``)
    }
  })

  it('rejects an omitted bundle resource', () => {
    const config = { bundle: { resources: { ...requiredResources } } }
    delete config.bundle.resources['../src/fonts/CommitMono-LICENSE.txt']

    expect(missingBundleResources(config, () => true)).toEqual(['../src/fonts/CommitMono-LICENSE.txt'])
  })

  it('rejects a missing resource file', () => {
    const missingFile = path.resolve(root, 'src/fonts/SchibstedGrotesk-OFL.txt')
    const exists = (file) => file !== missingFile
    const config = { bundle: { resources: { ...requiredResources } } }

    expect(missingBundleResources(config, exists)).toEqual(['../src/fonts/SchibstedGrotesk-OFL.txt'])
  })
})
