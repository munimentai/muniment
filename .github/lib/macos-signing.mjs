// The env contract for macOS Developer ID signing + notarization. These arrive
// through the SAME desktop-ci env-injection seam Windows signing uses (secrets
// injected at deploy from the vault, never committed). Absent every value => the
// build is unsigned, exactly as it ships today. Enrollment Y5DUNHQA74 is in
// review; the day it clears, only these six vault keys need filling.
//
//   APPLE_CERTIFICATE           base64 of the Developer ID Application .p12
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

// `security find-identity -v -p codesigning <keychain>` lists importable
// identities as `  1) <40-hex SHA-1>  "Developer ID Application: Name (TEAM)"`.
// Return the SHA-1 hash of the first Developer ID Application identity so
// codesign selects it unambiguously (a substring name match can collide when a
// keychain holds several certs). null when none is present.
export const parseSigningIdentity = (findIdentityOutput) => {
  const match = findIdentityOutput.match(/\b([0-9A-F]{40})\b\s+"(Developer ID Application:[^"]*)"/);
  return match ? { hash: match[1], name: match[2] } : null;
};

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

// notarytool authenticates with the App Store Connect API key (no Apple ID /
// app-specific password). `--wait` blocks until Apple returns Accepted/Invalid
// so a rejected build fails the job instead of shipping unnotarized.
export const notarytoolSubmitArguments = (configuration, archive, keyPath) => [
  "notarytool", "submit", archive,
  "--key", keyPath,
  "--key-id", configuration.apiKeyId,
  "--issuer", configuration.apiIssuer,
  "--wait",
];

// Staple the Apple ticket into the bundle so Gatekeeper validates offline.
export const stapleArguments = (bundle) => ["stapler", "staple", bundle];
