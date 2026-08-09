import { readFile, writeFile } from "node:fs/promises";

const [path, version, sha256] = process.argv.slice(2);

if (!path || !/^[0-9a-f]{40}$/.test(version ?? "") || !/^[0-9a-f]{64}$/.test(sha256 ?? "")) {
  throw new Error("usage: bump-homebrew-cask.mjs <path> <40-character source SHA> <SHA-256>");
}

const cask = await readFile(path, "utf8");
const versionPattern = /^  version "[0-9a-f]{40}"$/m;
const sha256Pattern = /^  sha256 "[0-9a-f]{64}"$/m;
const versionStanzas = cask.match(/^  version ".*"$/gm) ?? [];
const sha256Stanzas = cask.match(/^  sha256 ".*"$/gm) ?? [];
if (versionStanzas.length !== 1 || sha256Stanzas.length !== 1 ||
    !versionPattern.test(cask) || !sha256Pattern.test(cask)) {
  throw new Error(`${path} lacks one valid version or sha256 stanza`);
}

const updated = cask
  .replace(versionPattern, `  version "${version}"`)
  .replace(sha256Pattern, `  sha256 "${sha256}"`);
await writeFile(path, updated);
