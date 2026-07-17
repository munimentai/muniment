import { mkdtemp, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { createWingetManifest, machineMsiAsset, stableVersion } from "./winget-manifest.mjs";

const asset = { name: "nightly-sha-windows-muniment-machine.msi", url: "https://api.github.test/assets/1", browser_download_url: "https://github.com/mikeydiamonds/muniment-desktop/releases/download/v1.2.3/nightly-sha-windows-muniment-machine.msi" };
const release = { tag_name: "v1.2.3", draft: false, prerelease: false, published_at: "2026-07-17T12:34:56Z", assets: [asset] };

describe("WinGet stable manifest", () => {
  it("accepts only stable release tags", () => {
    expect(stableVersion("v1.2.3")).toBe("1.2.3");
    for (const tag of ["1.2.3", "v1.2.3-beta", "v01.2.3", "nightly"]) expect(() => stableVersion(tag)).toThrow("strict stable SemVer");
  });

  it("requires one machine MSI on a published stable release", () => {
    expect(machineMsiAsset(release)).toBe(asset);
    expect(() => machineMsiAsset({ ...release, prerelease: true })).toThrow("published stable");
    expect(() => machineMsiAsset({ ...release, assets: [] })).toThrow("found 0");
    expect(() => machineMsiAsset({ ...release, assets: [asset, asset] })).toThrow("found 2");
    expect(() => machineMsiAsset({ ...release, assets: [{ ...asset, browser_download_url: "https://example.test/file.msi" }] })).toThrow("public GitHub");
  });

  it("writes the per-machine singleton with the release URL and byte hash", async () => {
    const root = await mkdtemp(join(tmpdir(), "muniment-winget-"));
    const file = await createWingetManifest({ release, installerBytes: new TextEncoder().encode("msi bytes"), outputRoot: root });
    const manifest = await readFile(file, "utf8");
    expect(file).toContain(join("manifests", "m", "Muniment", "Muniment", "1.2.3"));
    expect(manifest).toContain("Publisher: Muniment");
    expect(manifest).toContain("InstallScope: machine");
    expect(manifest).toContain("ReleaseDate: 2026-07-17");
    expect(manifest).toContain(`InstallerUrl: ${asset.browser_download_url}`);
    expect(manifest).toMatch(/InstallerSha256: [A-F0-9]{64}/);
  });

  it("rejects empty installer downloads", async () => {
    await expect(createWingetManifest({ release, installerBytes: new Uint8Array(), outputRoot: tmpdir() })).rejects.toThrow("empty");
  });

  it("rejects a release without a publication date", async () => {
    await expect(createWingetManifest({ release: { ...release, published_at: null }, installerBytes: new Uint8Array([1]), outputRoot: tmpdir() })).rejects.toThrow("publication date");
  });
});
