import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { homedir, tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import {
  certificateSha1,
  codesignArguments,
  intermediateCertificateImportArguments,
  keychainSearchListArguments,
  NOTARIZATION_DEADLINE_SECONDS,
  notarize,
  parseInstallerIdentity,
  productbuildArguments,
  requireSigningIdentity,
  resolveSigningConfiguration,
  signingCertificateImportArguments,
  signingEnabled,
  signingIdentityArguments,
  signingKeyPartitionListArguments,
  stapleArguments,
} from "../../../.github/lib/macos-signing.mjs";


if (process.env.MUNIMENT_UPDATE_PROOF_DISPOSABLE !== "1") throw new Error("Disposable VM required");
const app = process.argv[2];
if (app !== "/Applications/muniment.app") throw new Error("Unexpected baseline path");
for (const line of readFileSync("/tmp/dci_env", "utf8").split(/\r?\n/)) {
  const eq = line.indexOf("=");
  if (eq > 0) process.env[line.slice(0, eq).trim()] = line.slice(eq + 1);
}
const signingConfig = resolveSigningConfiguration(process.env);
if (!signingConfig) throw new Error("Release signing credentials required");
const mustRun = (label, cmd, args) => {
  const result = spawnSync(cmd, args, { encoding: "utf8" });
  if (result.error || result.status !== 0) throw new Error(`${label} failed`);
};
const workDir = mkdtempSync(join(tmpdir(), "muniment-macos-signing-"));
const certPath = join(workDir, "certificate.p12");
const intermediateCertPath = join(".github", "certs", "DeveloperIDG2CA.cer");
const keyPath = join(workDir, "AuthKey.p8");
const keychain = join(workDir, "muniment-signing.keychain-db");
const keychainPassword = "muniment-ci-signing";
// trustd builds the signer's chain from the login keychain, not from a keychain the run
// adds to the search list, so the public Developer ID G2 intermediate sits in the login
// keychain for the run and leaves with it. The identity never does.
const loginKeychain = join(homedir(), "Library", "Keychains", "login.keychain-db");
const intermediateSha1 = certificateSha1(readFileSync(intermediateCertPath));
let intermediatePlaced = false;
const cleanup = () => {
  if (intermediatePlaced) spawnSync("security", ["delete-certificate", "-Z", intermediateSha1, loginKeychain]);
  spawnSync("security", ["delete-keychain", keychain]);
  rmSync(workDir, { recursive: true, force: true });
};
process.on("exit", cleanup);

writeFileSync(certPath, Buffer.from(signingConfig.certificate, "base64"), { mode: 0o600 });
writeFileSync(keyPath, Buffer.from(signingConfig.apiKey, "base64"), { mode: 0o600 });

// The throwaway keychain holds the identities for this run alone. Keep existing keychains
// and System Roots searchable so codesign can reach the Apple root.
mustRun("create keychain", "security", ["create-keychain", "-p", keychainPassword, keychain]);
mustRun("keychain settings", "security", ["set-keychain-settings", keychain]);
mustRun("unlock keychain", "security", ["unlock-keychain", "-p", keychainPassword, keychain]);
mustRun("import certificate", "security",
  signingCertificateImportArguments(certPath, keychain, signingConfig.certificatePassword));
const intermediatePresent = spawnSync("security",
  ["find-certificate", "-Z", "-c", "Developer ID Certification Authority", loginKeychain], { encoding: "utf8" });
if (intermediatePresent.error) throw intermediatePresent.error;
if ((intermediatePresent.stdout || "").includes(intermediateSha1)) {
  console.log("The login keychain already holds the Developer ID G2 intermediate. Skip the placement.");
} else {
  mustRun("place intermediate", "security", intermediateCertificateImportArguments(intermediateCertPath, loginKeychain));
  intermediatePlaced = true;
  console.log("placed intermediate: Developer ID Certification Authority, G2");
}
mustRun("authorize signing tools", "security",
  signingKeyPartitionListArguments(keychain, keychainPassword));
const priorKeychains = spawnSync("security", ["list-keychains", "-d", "user"], { encoding: "utf8" });
if (priorKeychains.error) throw priorKeychains.error;
if (priorKeychains.status !== 0) {
  console.error("Cannot read the keychain search list.");
  if (priorKeychains.stdout) process.stdout.write(priorKeychains.stdout);
  if (priorKeychains.stderr) process.stderr.write(priorKeychains.stderr);
  process.exit(priorKeychains.status ?? 1);
}
mustRun("register keychain", "security",
  keychainSearchListArguments(keychain, priorKeychains.stdout || ""));

const found = spawnSync("security", signingIdentityArguments(keychain), { encoding: "utf8" });
const identity = requireSigningIdentity(found);

const targetIdentity = spawnSync("codesign", ["-dv", "/tmp/muniment-updater-install-proof/expected/muniment.app"], { encoding: "utf8" });
if (targetIdentity.status !== 0 || !targetIdentity.stderr.includes(`TeamIdentifier=${signingConfig.teamId}`)) throw new Error("Target signer mismatch");
mustRun("sign baseline", "codesign", [...codesignArguments(identity.hash, app), "--entitlements", "src-tauri/packaging/entitlements.plist"]);
mustRun("verify baseline", "codesign", ["--verify", "--deep", "--strict", app]);
const archive = join(workDir, "baseline.zip");
mustRun("archive baseline", "ditto", ["-c", "-k", "--keepParent", app, archive]);
await notarize(signingConfig, archive, keyPath, performance.now() + 1200 * 1000);
mustRun("staple baseline", "xcrun", stapleArguments(app));
mustRun("validate staple", "xcrun", ["stapler", "validate", app]);
mustRun("assess baseline", "spctl", ["--assess", "--type", "execute", app]);
console.log("Baseline signed and notarized with the release identity");
