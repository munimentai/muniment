import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  SIGNING_VARIABLES,
  NOTARIZATION_DEADLINE_SECONDS,
  certificateSha1,
  codesignArguments,
  intermediateCertificateImportArguments,
  intermediateCertificateImportSucceeded,
  keychainSearchListArguments,
  notarytoolSubmitArguments,
  parseInstallerIdentity,
  parseSigningIdentity,
  productbuildArguments,
  requireSigningIdentity,
  resolveSigningConfiguration,
  signingCertificateImportArguments,
  signingEnabled,
  signingIdentityArguments,
  signingKeyPartitionListArguments,
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

describe("Keychain search list", () => {
  const roots = "/System/Library/Keychains/SystemRootCertificates.keychain";
  const keychain = "/tmp/signing work/muniment-signing.keychain-db";
  const login = "/Users/builder/Library/Keychains/login.keychain-db";

  it("prepends the signing keychain and adds System Roots to an empty list", () => {
    for (const currentList of ["", "\n  \r\n"]) {
      expect(keychainSearchListArguments(keychain, currentList)).toEqual([
        "list-keychains", "-d", "user", "-s", keychain, roots,
      ]);
    }
  });

  it("preserves existing keychains and paths with spaces", () => {
    expect(keychainSearchListArguments(keychain,
      `    "${login}"\r\n    "/Library/Keychains/System.keychain"\r\n    "/tmp/other signing.keychain-db"\r\n`,
    )).toEqual([
      "list-keychains", "-d", "user", "-s", keychain, login,
      "/Library/Keychains/System.keychain", "/tmp/other signing.keychain-db", roots,
    ]);
  });

  it("keeps one copy of each keychain when the list already includes System Roots", () => {
    expect(keychainSearchListArguments(keychain,
      `"${login}"\n"${keychain}"\n"${roots}"\n"${login}"\n`,
    )).toEqual(["list-keychains", "-d", "user", "-s", keychain, login, roots]);
  });
});

describe("Signing identity preflight", () => {
  const valid = '  1) A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4 "Developer ID Application: Muniment (Y5DUNHQA74)"';

  it("requests only valid code signing identities from the signing keychain", () => {
    expect(signingIdentityArguments("/tmp/signing work/keychain")).toEqual([
      "find-identity", "-v", "-p", "codesigning", "/tmp/signing work/keychain",
    ]);
  });

  it("accepts a valid identity after a successful command", () => {
    expect(requireSigningIdentity({ status: 0, stdout: `${valid}\r\n     1 valid identities found\r\n` }))
      .toEqual(parseSigningIdentity(valid));
  });

  it("names the chain check and includes stdout and stderr when no identity is valid", () => {
    const result = { status: 0, stdout: "     0 valid identities found", stderr: "Trust evaluation failed" };
    expect(() => requireSigningIdentity(result)).toThrow("Check the certificate chain and keychain access before codesign.");
    expect(() => requireSigningIdentity(result)).toThrow(result.stdout);
    expect(() => requireSigningIdentity(result)).toThrow(result.stderr);
  });

  it("rejects empty output and identities with a trust error", () => {
    for (const stdout of [undefined, null, "", `${valid} (CSSMERR_TP_NOT_TRUSTED)\n     0 valid identities found`]) {
      expect(() => requireSigningIdentity({ status: 0, stdout })).toThrow("No valid Developer ID Application identity.");
    }
  });

  it("rejects command failures even when stdout contains an identity", () => {
    for (const status of [1, 2, -1, null, undefined]) {
      expect(() => requireSigningIdentity({ status, stdout: valid, stderr: "security failed" }))
        .toThrow("security failed");
    }
    expect(() => requireSigningIdentity({ status: 0, stdout: valid, error: new Error("spawn failed") }))
      .toThrow("spawn failed");
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

describe("Intermediate certificate import results", () => {
  const duplicate = "security: SecKeychainItemImport: The specified item already exists in the keychain.\n";

  it("accepts a successful import", () => {
    expect(intermediateCertificateImportSucceeded({ status: 0, stdout: "1 certificate imported.\n" })).toBe(true);
    expect(intermediateCertificateImportSucceeded({ status: 0 })).toBe(true);
  });

  it("accepts exit code 1 only with the duplicate certificate message", () => {
    expect(intermediateCertificateImportSucceeded({ status: 1, stderr: duplicate })).toBe(true);
    expect(intermediateCertificateImportSucceeded({ status: 1, stdout: duplicate })).toBe(true);
    expect(intermediateCertificateImportSucceeded({ status: 1, stderr: "Cannot read certificate" })).toBe(false);
    expect(intermediateCertificateImportSucceeded({ status: 1, stdout: "", stderr: "" })).toBe(false);
    expect(intermediateCertificateImportSucceeded({ status: 1, stdout: null, stderr: null })).toBe(false);
  });

  it("rejects other exit codes and process failures even with the duplicate message", () => {
    for (const status of [2, -1, null, undefined]) {
      expect(intermediateCertificateImportSucceeded({ status, stderr: duplicate })).toBe(false);
    }
    expect(intermediateCertificateImportSucceeded({
      status: 1, stderr: duplicate, error: new Error("spawn failed"),
    })).toBe(false);
  });
});

describe("Signing key access", () => {
  const keychain = "/tmp/signing work/keychain";
  const password = 'p12 "pass"';

  it("trusts only codesign and productbuild during the private key import", () => {
    expect(signingCertificateImportArguments("/tmp/signing work/certificate.p12", keychain, password)).toEqual([
      "import", "/tmp/signing work/certificate.p12", "-k", keychain, "-P", password,
      "-T", "/usr/bin/codesign", "-T", "/usr/bin/productbuild",
    ]);
  });

  it("includes Apple, codesign, and productbuild partitions on the signing keys", () => {
    expect(signingKeyPartitionListArguments(keychain, password)).toEqual([
      "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:,productbuild:", "-s", "-k", password, keychain,
    ]);
  });
});

describe("Signing, notarization, and stapling commands", () => {
  it("keeps the vendored Developer ID G2 intermediate unchanged", () => {
    const certificate = readFileSync(".github/certs/DeveloperIDG2CA.cer");
    const fingerprint = createHash("sha256").update(certificate).digest("hex")
      .toUpperCase().match(/.{2}/g).join(":");
    expect(fingerprint).toBe(
      "F1:6C:D3:C5:4C:7F:83:CE:A4:BF:1A:3E:6A:08:19:C8:AA:A8:E4:A1:52:8F:D1:44:71:5F:35:06:43:D2:DF:3A",
    );
  });

  it("names a certificate by the upper-case SHA-1 of its DER bytes", () => {
    expect(certificateSha1(Buffer.from("abc"))).toBe("A9993E364706816ABA3E25717850C26C9CD0D89D");
  });

  it("builds the Developer ID G2 intermediate import arguments", () => {
    expect(intermediateCertificateImportArguments("DeveloperIDG2CA.cer", "/tmp/keychain")).toEqual([
      "import", "DeveloperIDG2CA.cer", "-k", "/tmp/keychain",
    ]);
  });

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

  it("reserves at least ten minutes before the desktop-ci build bound", () => {
    expect(NOTARIZATION_DEADLINE_SECONDS).toBeGreaterThan(0);
    expect(NOTARIZATION_DEADLINE_SECONDS).toBeLessThanOrEqual(3600 - 600);
  });

  it("submits with the App Store Connect API key and requests JSON without a wait", () => {
    const configuration = resolveSigningConfiguration(completeEnvironment);
    expect(notarytoolSubmitArguments(configuration, "muniment.app.zip", "/tmp/key.p8")).toEqual([
      "notarytool", "submit", "muniment.app.zip",
      "--key", "/tmp/key.p8",
      "--key-id", "ABC123DEF4",
      "--issuer", "57246542-96fe-1a63-e053-0824d011072a",
      "--output-format", "json", "--no-progress",
      "--no-wait",
    ]);
  });

  it("staples the notarization ticket into the bundle", () => {
    expect(stapleArguments("muniment.app")).toEqual(["stapler", "staple", "muniment.app"]);
  });
});
