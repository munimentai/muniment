import { describe, expect, it } from "vitest";
import {
  SIGNING_VARIABLES,
  codesignArguments,
  notarytoolSubmitArguments,
  parseInstallerIdentity,
  parseSigningIdentity,
  productbuildArguments,
  resolveSigningConfiguration,
  signingEnabled,
  stapleArguments,
} from "./macos-signing.mjs";

const completeEnvironment = {
  APPLE_CERTIFICATE: "cGtjczEy",
  APPLE_CERTIFICATE_PASSWORD: "p12 \"pass\"",
  APPLE_TEAM_ID: "Y5DUNHQA74",
  APPLE_API_KEY: "cDgtcHJpdmF0ZS1rZXk=",
  APPLE_API_KEY_ID: "ABC123DEF4",
  APPLE_API_ISSUER: "57246542-96fe-1a63-e053-0824d011072a",
};

describe("macOS signing configuration", () => {
  it("requires an explicit true flag", () => {
    for (const value of [undefined, "", "false"]) {
      expect(signingEnabled({ MACOS_SIGNING_ENABLED: value })).toBe(false);
    }
    expect(signingEnabled({ MACOS_SIGNING_ENABLED: "true" })).toBe(true);
    expect(() => signingEnabled({ MACOS_SIGNING_ENABLED: "TRUE" })).toThrow("must be true or false");
  });

  it("supports an unsigned build when no credentials are configured", () => {
    expect(resolveSigningConfiguration({})).toBeNull();
  });

  it("names every input the env contract expects", () => {
    expect(SIGNING_VARIABLES).toEqual([
      "APPLE_CERTIFICATE",
      "APPLE_CERTIFICATE_PASSWORD",
      "APPLE_TEAM_ID",
      "APPLE_API_KEY",
      "APPLE_API_KEY_ID",
      "APPLE_API_ISSUER",
    ]);
  });

  it("rejects a partial credential set and names only missing variables", () => {
    expect(() => resolveSigningConfiguration({
      APPLE_CERTIFICATE: "private cert value",
      APPLE_TEAM_ID: "Y5DUNHQA74",
    })).toThrow(
      "missing: APPLE_CERTIFICATE_PASSWORD, APPLE_API_KEY, APPLE_API_KEY_ID, APPLE_API_ISSUER",
    );
    try {
      resolveSigningConfiguration({ APPLE_CERTIFICATE: "do-not-print" });
    } catch (error) {
      expect(error.message).not.toContain("do-not-print");
    }
  });

  it("uses the complete credential set to create a signing configuration", () => {
    expect(resolveSigningConfiguration(completeEnvironment)).toEqual({
      certificate: completeEnvironment.APPLE_CERTIFICATE,
      certificatePassword: completeEnvironment.APPLE_CERTIFICATE_PASSWORD,
      teamId: completeEnvironment.APPLE_TEAM_ID,
      apiKey: completeEnvironment.APPLE_API_KEY,
      apiKeyId: completeEnvironment.APPLE_API_KEY_ID,
      apiIssuer: completeEnvironment.APPLE_API_ISSUER,
    });
  });
});

describe("Signing identity discovery", () => {
  it("extracts the Developer ID Application SHA-1 from find-identity output", () => {
    const output = [
      "Policy: Code Signing",
      "  Matching identities",
      '  1) A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4 "Developer ID Application: Muniment (Y5DUNHQA74)"',
      "     1 identities found",
    ].join("\n");
    expect(parseSigningIdentity(output)).toEqual({
      hash: "A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4",
      name: "Developer ID Application: Muniment (Y5DUNHQA74)",
    });
  });

  it("returns null when no Developer ID Application identity is present", () => {
    expect(parseSigningIdentity("     0 identities found")).toBeNull();
    expect(parseSigningIdentity(
      '  1) A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4 "Apple Development: Someone (TEAMID1234)"',
    )).toBeNull();
  });

  it("extracts the Developer ID Installer identity", () => {
    const output = '  2) B1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4 "Developer ID Installer: Muniment (Y5DUNHQA74)"';
    expect(parseInstallerIdentity(output)).toEqual({
      hash: "B1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4",
      name: "Developer ID Installer: Muniment (Y5DUNHQA74)",
    });
    expect(parseInstallerIdentity("0 valid identities found")).toBeNull();
  });
});

describe("Signing, notarization, and stapling commands", () => {
  it("builds unsigned and signed component package arguments", () => {
    expect(productbuildArguments("muniment.app", "muniment.pkg")).toEqual([
      "--component", "muniment.app", "/Applications", "muniment.pkg",
    ]);
    expect(productbuildArguments("muniment.app", "muniment.pkg", "B1B2", "/tmp/keychain")).toEqual([
      "--component", "muniment.app", "/Applications",
      "--sign", "B1B2", "--keychain", "/tmp/keychain", "muniment.pkg",
    ]);
  });

  it("codesigns with the hardened runtime and a secure timestamp", () => {
    expect(codesignArguments("A1B2C3D4", "muniment.app")).toEqual([
      "--force", "--options", "runtime", "--timestamp", "--sign", "A1B2C3D4", "muniment.app",
    ]);
  });

  it("submits to notarytool with the App Store Connect API key and waits", () => {
    const configuration = resolveSigningConfiguration(completeEnvironment);
    expect(notarytoolSubmitArguments(configuration, "muniment.app.zip", "/tmp/key.p8")).toEqual([
      "notarytool", "submit", "muniment.app.zip",
      "--key", "/tmp/key.p8",
      "--key-id", "ABC123DEF4",
      "--issuer", "57246542-96fe-1a63-e053-0824d011072a",
      "--wait",
    ]);
  });

  it("staples the notarization ticket into the bundle", () => {
    expect(stapleArguments("muniment.app")).toEqual(["stapler", "staple", "muniment.app"]);
  });
});
