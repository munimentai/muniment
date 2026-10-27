import { expect, it } from 'vitest'
import fs from 'node:fs'

it('keeps cargo usable with a minimal read-only toolchain while CI installs check components', () => {
  const toolchain = fs.readFileSync('rust-toolchain.toml', 'utf8')
  expect(toolchain).toMatch(/^channel = "1\.99\.0"$/m)
  expect(toolchain).not.toMatch(/^\s*components\s*=/m)
  for (const file of ['.github/workflows/ci.yml', '.forgejo/workflows/ci.yml']) {
    expect(fs.readFileSync(file, 'utf8')).toContain('"$HOME/.cargo/bin/rustup" component add rustfmt clippy')
  }
  for (const file of ['.forgejo/workflows/core.yml', '.forgejo/workflows/core-pins-update.yml']) {
    expect(fs.readFileSync(file, 'utf8')).toContain('bash ci/rust-toolchain.sh rustfmt clippy')
  }
})
