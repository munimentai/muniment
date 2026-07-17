import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

export const PACKAGE_IDENTIFIER = "Muniment.Muniment";

export const stableVersion = (tag) => {
  if (!/^v(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)$/.test(tag)) {
    throw new Error("release tag must be strict stable SemVer vMAJOR.MINOR.PATCH");
  }
  return tag.slice(1);
};

export const machineMsiAsset = (release) => {
  if (release.draft || release.prerelease) throw new Error("WinGet requires a published stable release");
  const matches = (release.assets ?? []).filter((asset) => asset.name.endsWith("-machine.msi"));
  if (matches.length !== 1) throw new Error(`expected exactly one per-machine MSI asset; found ${matches.length}`);
  if (!matches[0].browser_download_url?.startsWith("https://github.com/")) {
    throw new Error("per-machine MSI must have a public GitHub release URL");
  }
  return matches[0];
};

export const manifestText = ({ version, installerUrl, installerSha256, releaseDate }) => `# Created for the stable release by .github/lib/winget-release.mjs
PackageIdentifier: ${PACKAGE_IDENTIFIER}
PackageVersion: ${version}
PackageLocale: en-US
Publisher: Muniment
PackageName: muniment
License: Proprietary
ShortDescription: A private AI workspace for durable, governed work.
InstallerType: wix
InstallScope: machine
InstallModes:
  - interactive
  - silent
  - silentWithProgress
UpgradeBehavior: install
ReleaseDate: ${releaseDate}
Installers:
  - Architecture: x64
    InstallerUrl: ${installerUrl}
    InstallerSha256: ${installerSha256}
ManifestType: singleton
ManifestVersion: 1.12.0
`;

export async function createWingetManifest({ release, installerBytes, outputRoot }) {
  const version = stableVersion(release.tag_name);
  const asset = machineMsiAsset(release);
  const releaseDate = release.published_at?.slice(0, 10);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(releaseDate ?? "")) throw new Error("stable release is missing its publication date");
  if (!Number.isSafeInteger(installerBytes?.byteLength) || installerBytes.byteLength === 0) {
    throw new Error("downloaded per-machine MSI is empty");
  }
  const sha = createHash("sha256").update(installerBytes).digest("hex").toUpperCase();
  const directory = join(outputRoot, "manifests", "m", "Muniment", "Muniment", version);
  await mkdir(directory, { recursive: true });
  const file = join(directory, `${PACKAGE_IDENTIFIER}.yaml`);
  await writeFile(file, manifestText({ version, installerUrl: asset.browser_download_url, installerSha256: sha, releaseDate }));
  return file;
}
