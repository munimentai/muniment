import { expect, it } from 'vitest'
import { fileIcon } from './file-icon.js'
it('distinguishes common file types and honors exact names and compound suffixes', () => {
  expect(fileIcon('one.py')).not.toEqual(fileIcon('one.js'))
  expect(fileIcon('one.py')).toEqual(fileIcon('TWO.PY'))
  expect(fileIcon('one.json')).not.toEqual(fileIcon('one.csv'))
  expect(fileIcon('a.test.js')).not.toEqual(fileIcon('a.js'))
  expect(fileIcon('Dockerfile')).not.toEqual(fileIcon('unknown'))
  expect(fileIcon('.gitignore')).not.toEqual(fileIcon('a.json'))
  expect(fileIcon('unknown').character).toHaveLength(1)
})
