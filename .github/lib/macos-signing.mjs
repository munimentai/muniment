import { spawnSync } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";

// The env contract for macOS Developer ID signing + notarization. These arrive
// through the SAME desktop-ci env-injection seam Windows signing uses (secrets
// injected at deploy from the vault, never committed). The config flag stays
// false while enrollment Y5DUNHQA74 remains in review.
//
//   APPLE_CERTIFICATE           base64 of a .p12 with Application and Installer identities
//   APPLE_CERTIFICATE_PASSWORD  password protecting that .p12
//   APPLE_TEAM_ID               10-character Apple Developer Team ID
//   APPLE_API_KEY               base64 of the App Store Connect .p8 key
//   APPLE_API_KEY_ID            the key's identifier (the "Key ID" column)
//   APPLE_API_ISSUER            the App Store Connect issuer UUID
export const SIGNING_VARIABLES = [
  "APPLE_CERTIFICATE",
  "APPLE_CERTIFICATE_PASSWORD",
  "APPLE_TEAM_ID",
  "APPLE_API_KEY",
  "APPLE_API_KEY_ID",
  "APPLE_API_ISSUER",
];

export const signingEnabled = (env) => {
  const value = env.MACOS_SIGNING_ENABLED;
  if (value === undefined || value === "" || value === "false") return false;
  if (value === "true") return true;
  throw new Error("MACOS_SIGNING_ENABLED must be true or false");
};

// null when no credentials are present (unsigned build); a resolved config when
// all are present; throws naming only the MISSING variables when the set is
// partial (never echoing a secret value), mirroring the Windows resolver.
export const resolveSigningConfiguration = (env) => {
  const present = SIGNING_VARIABLES.filter((name) => Boolean(env[name]));
  if (present.length === 0) return null;

  const missing = SIGNING_VARIABLES.filter((name) => !env[name]);
  if (missing.length > 0) {
    throw new Error(`incomplete macOS signing configuration; missing: ${missing.join(", ")}`);
  }

  return {
    certificate: env.APPLE_CERTIFICATE,
    certificatePassword: env.APPLE_CERTIFICATE_PASSWORD,
    teamId: env.APPLE_TEAM_ID,
    apiKey: env.APPLE_API_KEY,
    apiKeyId: env.APPLE_API_KEY_ID,
    apiIssuer: env.APPLE_API_ISSUER,
  };
};

export const keychainSearchListArguments = (keychain, currentList) => {
  const currentKeychains = currentList.split(/\r?\n/)
    .map((line) => line.trim().replace(/^"|"$/g, "")).filter(Boolean);
  return ["list-keychains", "-d", "user", "-s", ...new Set([
    keychain,
    ...currentKeychains,
    "/System/Library/Keychains/SystemRootCertificates.keychain",
  ])];
};

export const signingIdentityArguments = (keychain) => [
  "find-identity", "-v", "-p", "codesigning", keychain,
];

// `security find-identity -v -p codesigning <keychain>` lists valid
// identities as `  1) <40-hex SHA-1>  "Developer ID Application: Name (TEAM)"`.
// Return the SHA-1 hash of the first Developer ID Application identity so
// codesign selects it unambiguously (a substring name match can collide when a
// keychain holds several certs). null when none is present.
export const parseSigningIdentity = (findIdentityOutput) => {
  const match = findIdentityOutput.match(/^\s*\d+\) ([0-9A-F]{40})[ \t]+"(Developer ID Application:[^"\r\n]+)"[ \t]*\r?$/m);
  return match ? { hash: match[1], name: match[2] } : null;
};

export const requireSigningIdentity = (result) => {
  const identity = parseSigningIdentity(result.stdout || "");
  if (result.error || result.status !== 0 || !identity) {
    throw new Error([
      "No valid Developer ID Application identity. Check the certificate chain and keychain access before codesign.",
      `security find-identity exited with status ${result.status ?? "unknown"}.`,
      result.error?.message,
      result.stdout,
      result.stderr,
    ].filter(Boolean).join("\n"));
  }
  return identity;
};

export const parseInstallerIdentity = (findIdentityOutput) => {
  const match = findIdentityOutput.match(/\b([0-9A-F]{40})\b\s+"(Developer ID Installer:[^"]*)"/);
  return match ? { hash: match[1], name: match[2] } : null;
};

export const productbuildArguments = (application, output, identityHash, keychain) => {
  const args = ["--component", application, "/Applications"];
  if (identityHash) args.push("--sign", identityHash, "--keychain", keychain);
  return [...args, output];
};

export const signingCertificateImportArguments = (certificate, keychain, password) => [
  "import", certificate, "-k", keychain, "-P", password,
  "-T", "/usr/bin/codesign", "-T", "/usr/bin/productbuild",
];

// Apple partitions cover codesign and productbuild. Tool paths belong in the import ACL, not the partition list.
export const signingKeyPartitionListArguments = (keychain, password) => [
  "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s", "-k", password, keychain,
];

export const intermediateCertificateImportArguments = (certificate, keychain) => [
  "import", certificate, "-k", keychain,
];

export const intermediateCertificateImportSucceeded = (result) =>
  !result.error && (result.status === 0 || (result.status === 1 &&
    [result.stdout, result.stderr].some((output) =>
      (output || "").includes("already exists in the keychain"))));

// Deep-sign a bundle (or a lone binary) with the hardened runtime and a secure
// timestamp — both are notarization prerequisites. Inner Mach-O resources are
// signed first (deepest last would be re-sealed), so `identity` is applied to
// each path the caller passes in leaf-to-root order.
export const codesignArguments = (identityHash, file) => [
  "--force",
  "--options", "runtime",
  "--timestamp",
  "--sign", identityHash,
  file,
];

// Cap both submissions and compilation together. The remaining desktop-ci budget can shorten this bound.
export const NOTARIZATION_DEADLINE_SECONDS = 2400;

const notarytoolArguments = (configuration, keyPath) => [
  "--key", keyPath,
  "--key-id", configuration.apiKeyId,
  "--issuer", configuration.apiIssuer,
  "--output-format", "json", "--no-progress",
];

export const notarytoolSubmitArguments = (configuration, archive, keyPath) => [
  "notarytool", "submit", archive,
  ...notarytoolArguments(configuration, keyPath),
  "--no-wait",
];

// Poll structured status so a stalled queue leaves the submission id and last status in the build log.
export const notarize = async (configuration, archive, keyPath, deadline) => {
  const started = performance.now();
  let submissionId = "unknown";
  let lastStatus = "unknown";
  const fail = (cause) => {
    const waited = Math.ceil((performance.now() - started) / 1000);
    throw new Error(`macOS notarization FAILED cause=${cause} submission_id=${submissionId} last_status=${JSON.stringify(lastStatus)} waited_seconds=${waited} archive=${JSON.stringify(archive)}`);
  };
  if (!Number.isFinite(deadline)) fail("invalid-deadline");
  const remaining = () => deadline - performance.now();
  const request = (args, timeout) => {
    if (remaining() <= 0) fail("notarization-timeout");
    const result = spawnSync("xcrun", args, {
      encoding: "utf8",
      timeout: Math.max(1, Math.ceil(Math.min(timeout, remaining()))),
      killSignal: "SIGKILL",
    });
    if (result.error || result.status !== 0) {
      if (result.stdout) process.stdout.write(result.stdout);
      if (result.stderr) process.stderr.write(result.stderr);
      fail(remaining() <= 0 ? "notarization-timeout" : "notarytool-failed");
    }
    try {
      return JSON.parse(result.stdout);
    } catch {
      fail("invalid-response");
    }
  };
  const submission = request(notarytoolSubmitArguments(configuration, archive, keyPath), 300_000);
  if (typeof submission?.id !== "string" ||
      !/^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(submission.id)) fail("invalid-response");
  submissionId = submission.id;
  console.log(`macOS notarization submission_id=${submissionId} archive=${JSON.stringify(archive)}`);
  while (true) {
    const info = request([
      "notarytool", "info", submissionId, ...notarytoolArguments(configuration, keyPath),
    ], 60_000);
    if (info?.id !== submissionId ||
        !["In Progress", "Accepted", "Invalid", "Rejected"].includes(info.status)) fail("invalid-response");
    lastStatus = info.status;
    if (remaining() <= 0) fail("notarization-timeout");
    if (lastStatus === "Accepted") return;
    if (lastStatus !== "In Progress") fail("notarization-rejected");
    await sleep(Math.min(15_000, remaining()));
  }
};

// Staple the Apple ticket into the bundle so Gatekeeper validates offline.
export const stapleArguments = (bundle) => ["stapler", "staple", bundle];
