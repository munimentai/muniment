import { describe, it, expect } from 'vitest';
import { createUpdateFeed } from './update-feed.mjs';
const names = ['release.AppImage', 'release.app.tar.gz', 'release-arm64.app.tar.gz', 'release-x64.app.tar.gz', 'release.msi', 'release-machine.msi', 'release-nsis.exe'];
const input = () => ({ repository: 'munimentai/muniment', version: 'v0.0.1', assets: names.map(name => ({ name })), signatures: new Map(names.map(name => [`${name}.sig`, Buffer.from(`signature:${name}`).toString('base64')])) });
describe('signed update feed', () => {
  it('keeps installer formats and scopes separate and uses stable URLs', () => {
    const feed = createUpdateFeed(input());
    expect(feed.version).toBe('0.0.1');
    expect(feed.platforms['darwin-aarch64'].url).toMatch(/-arm64.app.tar.gz$/);
    expect(feed.platforms['darwin-x86_64'].url).toMatch(/-x64.app.tar.gz$/);
    for (const [target, file] of [['linux-x86_64', 'release.AppImage'], ['windows-x86_64-msi-user', 'release.msi'], ['windows-x86_64-msi-machine', 'release-machine.msi'], ['windows-x86_64-nsis', 'release-nsis.exe']]) {
      expect(feed.platforms[target].url).toBe(`https://github.com/munimentai/muniment/releases/download/v0.0.1/${file}`);
      expect(feed.platforms[target].signature).toBe(input().signatures.get(`${file}.sig`));
    }
  });
  it('rejects a missing signature instead of advertising an unverifiable update', () => {
    const data = input(); data.signatures.delete('release.AppImage.sig');
    expect(() => createUpdateFeed(data)).toThrow('signature');
  });
  it('rejects duplicate artifacts and invalid release identities', () => {
    const data = input(); data.assets.push(data.assets[0]);
    expect(() => createUpdateFeed(data)).toThrow('Expected one');
    expect(() => createUpdateFeed({ ...input(), version: 'v0.0.1-rc' })).toThrow('identity');
    expect(() => createUpdateFeed({ ...input(), repository: 'owner/repo/extra' })).toThrow('identity');
  });
});
