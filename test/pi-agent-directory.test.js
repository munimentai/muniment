import { expect, it } from 'vitest'
import { piAgentDirectory } from './e2e/support/pi-agent-directory.mjs'

it('matches Pi directory overrides on POSIX', () => {
  for (const [input, expected] of [
    ['', '/home/tester/.pi/agent'],
    ['~', '/home/tester'],
    ['~/agent', '/home/tester/agent'],
    ['~//agent', '/home/tester/agent'],
    ['~/nested/../agent', '/home/tester/agent'],
    ['~\\agent', '~\\agent'],
    ['relative/agent', 'relative/agent'],
    [' spaced ', ' spaced '],
    ['file:///tmp/agent%20files', '/tmp/agent files'],
  ]) expect(piAgentDirectory(input, '/home/tester', false)).toBe(expected)
  expect(() => piAgentDirectory('file://remote/agent', '/home/tester', false)).toThrow()
  expect(() => piAgentDirectory('file:///tmp/a%2Fb', '/home/tester', false)).toThrow()
})

it('matches Pi directory overrides on Windows', () => {
  for (const [input, expected] of [
    ['', 'C:\\Users\\tester\\.pi\\agent'],
    ['~\\agent', 'C:\\Users\\tester\\agent'],
    ['~\\\\agent', 'C:\\Users\\tester\\agent'],
    ['~/agent', 'C:\\Users\\tester\\agent'],
    ['/c/agent', 'C:\\agent'],
    ['/mnt/d/agent/sub', 'D:\\agent\\sub'],
    ['/MNT/d/agent', 'D:\\agent'],
    ['/cygdrive/E', 'E:\\'],
    ['//server/share', '//server/share'],
    ['/home/agent', '/home/agent'],
    ['/c/a\\b', '/c/a\\b'],
    ['/mnt/cc/agent', '/mnt/cc/agent'],
    ['file:///C:/agent%20files', 'C:\\agent files'],
    ['file://server/share', '\\\\server\\share'],
  ]) expect(piAgentDirectory(input, 'C:\\Users\\tester', true)).toBe(expected)
  expect(() => piAgentDirectory('file:///C:/a%5Cb', 'C:\\Users\\tester', true)).toThrow()
})
