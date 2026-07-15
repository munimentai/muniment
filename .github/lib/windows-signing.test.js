import { describe, expect, it } from "vitest";
import {
  SIGN_CLI_VERSION,
  resolveSigningConfiguration,
  signArguments,
  signToolInstallArgs,
  tauriSignCommand,
} from "./windows-signing.mjs";

const completeEnvironment = {
  AZURE_TENANT_ID: "tenant",
  AZURE_CLIENT_ID: "client",
  AZURE_CLIENT_SECRET: "secret",
  AZURE_SIGNING_ENDPOINT: "https://example.test/signing endpoint",
  AZURE_SIGNING_ACCOUNT: "Muniment Account",
  AZURE_SIGNING_PROFILE: "Desktop \"Release\"",
};

describe("Windows signing configuration", () => {
  it("supports an unsigned build when no credentials are configured", () => {
    expect(resolveSigningConfiguration({})).toBeNull();
  });

  it("rejects a partial credential set and names only missing variables", () => {
    expect(() => resolveSigningConfiguration({
      AZURE_TENANT_ID: "private tenant value",
      AZURE_CLIENT_ID: "private client value",
    })).toThrow(
      "missing: AZURE_CLIENT_SECRET, AZURE_SIGNING_ENDPOINT, AZURE_SIGNING_ACCOUNT, AZURE_SIGNING_PROFILE",
    );
    try {
      resolveSigningConfiguration({ AZURE_TENANT_ID: "do-not-print" });
    } catch (error) {
      expect(error.message).not.toContain("do-not-print");
    }
  });

  it("uses the complete credential set to create Artifact Signing options", () => {
    expect(resolveSigningConfiguration(completeEnvironment)).toEqual({
      timestampUrl: "http://timestamp.acs.microsoft.com",
      endpoint: completeEnvironment.AZURE_SIGNING_ENDPOINT,
      account: completeEnvironment.AZURE_SIGNING_ACCOUNT,
      certificateProfile: completeEnvironment.AZURE_SIGNING_PROFILE,
    });
  });
});

describe("Sign CLI commands", () => {
  it("installs the exact reviewed prerelease", () => {
    expect(signToolInstallArgs("C:\\sign tools")).toEqual([
      "tool", "install", "--tool-path", "C:\\sign tools",
      "--version", SIGN_CLI_VERSION,
      "sign",
    ]);
    expect(SIGN_CLI_VERSION).toBe("0.9.1-beta.26330.1");
  });

  it("constructs equivalent direct and Tauri signing arguments", () => {
    const configuration = resolveSigningConfiguration(completeEnvironment);
    expect(signArguments(configuration, "artifact.msi")).toEqual([
      "code", "artifact-signing", "--verbosity", "warning",
      "--timestamp-url", "http://timestamp.acs.microsoft.com",
      "--artifact-signing-endpoint", completeEnvironment.AZURE_SIGNING_ENDPOINT,
      "--artifact-signing-account", completeEnvironment.AZURE_SIGNING_ACCOUNT,
      "--artifact-signing-certificate-profile", completeEnvironment.AZURE_SIGNING_PROFILE,
      "artifact.msi",
    ]);
    expect(tauriSignCommand(configuration)).toEqual({
      cmd: "sign",
      args: [
        "code", "artifact-signing", "--verbosity", "warning",
        "--timestamp-url", "http://timestamp.acs.microsoft.com",
        "--artifact-signing-endpoint", "https://example.test/signing endpoint",
        "--artifact-signing-account", "Muniment Account",
        "--artifact-signing-certificate-profile", "Desktop \"Release\"",
        "%1",
      ],
    });
  });
});
