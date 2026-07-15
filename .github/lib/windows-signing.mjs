export const SIGN_CLI_VERSION = "0.9.1-beta.26330.1";

export const SIGNING_VARIABLES = [
  "AZURE_TENANT_ID",
  "AZURE_CLIENT_ID",
  "AZURE_CLIENT_SECRET",
  "AZURE_SIGNING_ENDPOINT",
  "AZURE_SIGNING_ACCOUNT",
  "AZURE_SIGNING_PROFILE",
];

export const signToolInstallArgs = (toolPath) => [
  "tool", "install", "--tool-path", toolPath,
  "--version", SIGN_CLI_VERSION,
  "sign",
];

export const resolveSigningConfiguration = (env) => {
  const present = SIGNING_VARIABLES.filter((name) => Boolean(env[name]));
  if (present.length === 0) return null;

  const missing = SIGNING_VARIABLES.filter((name) => !env[name]);
  if (missing.length > 0) {
    throw new Error(`incomplete Windows signing configuration; missing: ${missing.join(", ")}`);
  }

  return {
    timestampUrl: "http://timestamp.acs.microsoft.com",
    endpoint: env.AZURE_SIGNING_ENDPOINT,
    account: env.AZURE_SIGNING_ACCOUNT,
    certificateProfile: env.AZURE_SIGNING_PROFILE,
  };
};

export const signArguments = (configuration, file) => [
  "code", "artifact-signing",
  "--verbosity", "warning",
  "--timestamp-url", configuration.timestampUrl,
  "--artifact-signing-endpoint", configuration.endpoint,
  "--artifact-signing-account", configuration.account,
  "--artifact-signing-certificate-profile", configuration.certificateProfile,
  file,
];

export const tauriSignCommand = (configuration) =>
  ({ cmd: "sign", args: signArguments(configuration, "%1") });
