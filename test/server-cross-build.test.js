import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const root = process.cwd()
const read = (file) => fs.readFileSync(path.join(root, file), 'utf8')
const smoke = read('test/smoke.sh')
const crossBuild = read('core/ci/server-cross-build.sh')
const packageScript = read('core/deploy/runtime/package.sh')
const libraries = read('core/deploy/runtime/server-libraries.sh')
const releaseAssets = read('core/deploy/runtime/release-assets.sh')

describe('aarch64 server cross-build in PR smoke', () => {
  it('runs the cross-build once, unguarded, in the structure smoke', () => {
    const calls = smoke
      .split('\n')
      .filter((line) => line.includes('server-cross-build.sh'))
      .map((line) => line.trim())
    expect(calls).toEqual(['core/ci/server-cross-build.sh'])
    expect(smoke).toContain(
      'if [ -n "${GITHUB_ACTIONS:-}" ] || [ "${MUNIMENT_SERVER_CROSS_BUILD:-}" = 1 ]; then'
    )
    expect(smoke).toContain('python3 -B core/deploy/runtime/test_package.py')
  })

  it('cross-links through the release packaging path with the pinned zig tools', () => {
    expect(crossBuild).toContain('source "$context/ci/zig-toolchain.sh"')
    expect(crossBuild).toContain('"$context/deploy/runtime/package.sh" "$version" aarch64 "$out"')
    expect(crossBuild).not.toMatch(/cargo (check|build)\b/)
    expect(crossBuild).not.toContain('sign-release.sh')
    expect(crossBuild).toContain('[ "$machine" != 183 ]')
  })

  it('links each target against its own pinned libraries with a real release build', () => {
    expect(packageScript).toContain('cargo zigbuild "${library_args[@]}" --release --locked')
    expect(packageScript).toContain('--target "$target.2.36"')
    expect(packageScript).toContain('"$libraries" check-archive "$target" "$archive"')
    expect(packageScript).not.toContain('"$built"/*.so*')
    expect(libraries).toContain(
      'SHERPA_ONNX_LIB_DIR = { value = "%s", force = true, relative = false }'
    )
    expect(libraries).toContain("printf '%s\\n' --config \"$3\"")
    expect(libraries).not.toContain('--config env.')
    expect(libraries).not.toContain('src-tauri')
    expect(packageScript).not.toContain('src-tauri')
  })

  it('checks the package arguments with the installed cargo zigbuild before the link', () => {
    const check = crossBuild.indexOf(
      'MUNIMENT_REQUIRE_ZIGBUILD=1 python3 -B "$context/deploy/runtime/test_package.py" ZigbuildPrecedence'
    )
    const toolchain = crossBuild.indexOf('source "$context/ci/zig-toolchain.sh"')
    const link = crossBuild.indexOf('"$context/deploy/runtime/package.sh"')
    expect(toolchain).toBeGreaterThan(0)
    expect(check).toBeGreaterThan(toolchain)
    expect(link).toBeGreaterThan(check)
  })

  it('builds both release archives before the signing step', () => {
    const build = releaseAssets.indexOf('"$context/deploy/runtime/package.sh"')
    const sign = releaseAssets.indexOf('"$context/deploy/runtime/sign-release.sh"')
    expect(build).toBeGreaterThan(0)
    expect(sign).toBeGreaterThan(build)
    expect(releaseAssets).toContain('set -euo pipefail')
  })
})
