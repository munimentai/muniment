import { describe, expect, it } from 'vitest'
import fs from 'node:fs'

const attributes = fs.readFileSync('.gitattributes', 'utf8').split(/\r?\n/)

describe('line ending attributes', () => {
  it('checks out every text file with LF line endings', () => {
    expect(attributes).toContain('* text=auto eol=lf')
    expect(attributes).toContain('protocol-fixtures/**/*.json text eol=lf')
  })

  it.each([
    'png',
    'woff2',
    'otf',
    'ttf',
    'ico',
    'icns',
    'dll',
    'dylib',
    'so',
    'lib',
    'onnx',
  ])('does not convert %s files', (extension) => {
    expect(attributes).toContain(`*.${extension} binary`)
  })
})
