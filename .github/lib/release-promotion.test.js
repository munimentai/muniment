import { describe, expect, it } from "vitest";
import { assertGreenCi, expectedNightlyAssets, releaseBody, validatePromotionInputs } from "./release-promotion.mjs";

const sha = "a".repeat(40);
const assets = [
  { name: `nightly-${sha}-linux-muniment.deb` },
  { name: `nightly-${sha}-linux-muniment.AppImage` },
  { name: `nightly-${sha}-windows-muniment.msi` },
  { name: `nightly-${sha}-windows-muniment-machine.msi` },
  { name: `nightly-${sha}-windows-muniment-nsis.exe` },
  { name: `nightly-${sha}-macos-muniment.app.zip` },
];

describe("stable release promotion", () => {
  it("accepts only exact lowercase SHAs and strict stable SemVer tags", () => {
    expect(() => validatePromotionInputs(sha, "v1.2.3")).not.toThrow();
    for (const invalid of ["1.2.3", "v1.2", "v01.2.3", "v1.2.3-beta", "v1.2.3 "]) {
      expect(() => validatePromotionInputs(sha, invalid)).toThrow("strict SemVer");
    }
    for (const invalid of ["a".repeat(39), "A".repeat(40), `${sha}0`, "g".repeat(40)]) {
      expect(() => validatePromotionInputs(invalid, "v1.2.3")).toThrow("40 lowercase");
    }
  });

  it("requires exactly one of each finalized nightly artifact", () => {
    expect(expectedNightlyAssets(assets, sha)).toEqual(assets);
    expect(() => expectedNightlyAssets(assets.slice(1), sha)).toThrow("exactly six");
    expect(() => expectedNightlyAssets([...assets.slice(0, 5), assets[0]], sha)).toThrow("Linux deb");
    expect(() => expectedNightlyAssets([...assets, { name: `nightly-${sha}-extra.txt` }], sha)).toThrow("exactly six");
  });

  it("requires a successful smoke check and rejects a failed CI job", () => {
    const smoke = { name: "smoke", status: "completed", conclusion: "success" };
    expect(() => assertGreenCi([smoke], sha)).not.toThrow();
    expect(() => assertGreenCi([], sha)).toThrow("CI is not green");
    expect(() => assertGreenCi([{ ...smoke, status: "in_progress", conclusion: null }], sha)).toThrow("CI is not green");
    expect(() => assertGreenCi([smoke, { name: "desktop-build (linux)", status: "completed", conclusion: "failure" }], sha)).toThrow("CI is not green");
  });

  it("publishes the required provenance and signing disclosures", () => {
    const body = releaseBody(sha);
    expect(body).toContain(sha);
    expect(body).toContain("Windows installers are signed");
    expect(body).toContain("macOS artifacts are unsigned pending Apple credentials");
    expect(body).toContain("Model weights are not included");
  });
});
