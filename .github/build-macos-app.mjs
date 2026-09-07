import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { readdir } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import {
  codesignArguments,
  intermediateCertificateImportArguments,
  intermediateCertificateImportSucceeded,
  keychainSearchListArguments,
  NOTARIZATION_DEADLINE_SECONDS,
  notarize,
  parseInstallerIdentity,
  productbuildArguments,
  requireSigningIdentity,
  resolveSigningConfiguration,
  signingEnabled,
  signingIdentityArguments,
  stapleArguments,
} from "./lib/macos-signing.mjs";

const remainingBuildBudget = process.env.MACOS_BUILD_REMAINING_SECONDS ?? "3600";
const remainingBuildSeconds = Number(remainingBuildBudget);
if (!/^-?\d+$/.test(remainingBuildBudget) || !Number.isSafeInteger(remainingBuildSeconds) || remainingBuildSeconds > 3600) {
  throw new Error("MACOS_BUILD_REMAINING_SECONDS must be an integer at most 3600");
}
// Keep 1200 seconds of the full desktop-ci build budget for failure reporting and process startup.
const notarizationDeadline = performance.now() + (
  remainingBuildSeconds - (3600 - NOTARIZATION_DEADLINE_SECONDS)
) * 1000;

const bundleDir = join("src-tauri", "target", "universal-apple-darwin", "release", "bundle", "macos");
const app = join(bundleDir, "muniment.app");
const runtime = join(app, "Contents", "Library", "LaunchServices", "muniment-runtime");
const appZip = `${app}.zip`;
const pkgDir = join(bundleDir, "..", "pkg");
const pkg = join(pkgDir, "muniment.pkg");

const tauri = (...args) => {
  const cli = join("node_modules", "@tauri-apps", "cli", "tauri.js");
  const result = spawnSync(process.execPath, [cli, ...args], { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};

// Fail loudly with the tool's own output rather than a generic message.
const mustRun = (label, cmd, args) => {
  const result = spawnSync(cmd, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    console.error(`${label} FAILED (${cmd} rc=${result.status}) — see output above`);
    process.exit(result.status ?? 1);
  }
};

// Package the .app into the .app.zip release asset. macOS-specific ditto flags
// preserve the bundle's symlinks and resource forks so the archive extracts to a
// launchable (and, when signed, still-notarized) bundle.
const packageApp = () =>
  mustRun("bundle archive", "ditto", ["-c", "-k", "--sequesterRsrc", "--keepParent", app, appZip]);

// Apple Developer ID credentials arrive as a file the desktop-ci driver writes
// into the VM over the SSH data channel (never on argv) — the SAME env-injection
// seam Windows signing uses. Load them so the signing config below can resolve.
// The config flag controls signing. The nightly keeps shipping unsigned while
// enrollment Y5DUNHQA74 remains in review.
const credFile = join(tmpdir(), "dci_env");
if (existsSync(credFile)) {
  for (const line of readFileSync(credFile, "utf8").split(/\r?\n/)) {
    const eq = line.indexOf("=");
    if (eq > 0) process.env[line.slice(0, eq).trim()] = line.slice(eq + 1);
  }
}
// Resolve credentials only when the flag enables signing.
const signing = signingEnabled(process.env);
const signingConfig = signing ? resolveSigningConfiguration(process.env) : null;
if (signing && signingConfig === null) {
  throw new Error("MACOS_SIGNING_ENABLED is true but Apple credentials are absent");
}

// Build the universal app without Tauri signing so this script checks trust before the first codesign call.
mustRun("build universal runtime", process.execPath, [join(".github", "build-macos-runtime.mjs")]);
tauri("build", "--target", "universal-apple-darwin", "--bundles", "app", "--no-sign");

if (!signing) {
  console.log("macOS signing SKIPPED: MACOS_SIGNING_ENABLED is false (unsigned build)");
  packageApp();
  mustRun("make package directory", "mkdir", ["-p", pkgDir]);
  mustRun("build installer", "productbuild", productbuildArguments(app, pkg));
  console.log(`kept ${appZip} (unsigned)`);
  console.log(`kept ${pkg} (unsigned)`);
  process.exit(0);
}

console.log("macOS signing ENABLED (Developer ID codesign + notarytool notarization + staple)");

// Everything secret-bearing lives in a throwaway directory removed on exit.
const workDir = mkdtempSync(join(tmpdir(), "muniment-macos-signing-"));
const certPath = join(workDir, "certificate.p12");
const intermediateCertPath = join(".github", "certs", "DeveloperIDG2CA.cer");
const keyPath = join(workDir, "AuthKey.p8");
const keychain = join(workDir, "muniment-signing.keychain-db");
const keychainPassword = "muniment-ci-signing";
const cleanup = () => rmSync(workDir, { recursive: true, force: true });
process.on("exit", cleanup);

writeFileSync(certPath, Buffer.from(signingConfig.certificate, "base64"), { mode: 0o600 });
writeFileSync(keyPath, Buffer.from(signingConfig.apiKey, "base64"), { mode: 0o600 });

// The dedicated keychain holds the Developer ID identities and G2 intermediate.
// Keep existing keychains and System Roots searchable so codesign can reach the Apple root.
mustRun("create keychain", "security", ["create-keychain", "-p", keychainPassword, keychain]);
mustRun("keychain settings", "security", ["set-keychain-settings", keychain]);
mustRun("unlock keychain", "security", ["unlock-keychain", "-p", keychainPassword, keychain]);
mustRun("import certificate", "security",
  ["import", certPath, "-k", keychain, "-P", signingConfig.certificatePassword, "-T", "/usr/bin/codesign"]);
const intermediate = spawnSync("security",
  ["find-certificate", "-c", "Developer ID Certification Authority", keychain], { encoding: "utf8" });
if (intermediate.error) throw intermediate.error;
if (intermediate.status === 0) {
  console.log("The keychain already holds the Developer ID G2 intermediate. Skip the import.");
} else {
  const imported = spawnSync("security",
    intermediateCertificateImportArguments(intermediateCertPath, keychain), { encoding: "utf8" });
  if (imported.stdout) process.stdout.write(imported.stdout);
  if (imported.stderr) process.stderr.write(imported.stderr);
  if (imported.error) throw imported.error;
  if (!intermediateCertificateImportSucceeded(imported)) {
    console.error(`import Developer ID G2 intermediate FAILED (security rc=${imported.status}) — see output above`);
    process.exit(imported.status ?? 1);
  }
}
mustRun("show Developer ID G2 intermediate", "security",
  ["find-certificate", "-c", "Developer ID Certification Authority", keychain]);
console.log("imported intermediate: Developer ID Certification Authority, G2");
mustRun("authorize codesign", "security",
  ["set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s", "-k", keychainPassword, keychain]);
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
const installerIdentities = spawnSync("security", ["find-identity", "-v", keychain], { encoding: "utf8" });
const installerIdentity = parseInstallerIdentity(installerIdentities.stdout || "");
if (!installerIdentity) {
  console.error("no Developer ID Installer identity found in the imported certificate");
  process.exit(1);
}
console.log(`signing identity: ${identity.name}`);

// Sign nested Mach-O resources (the ASR runtime dylibs) leaf-first, then seal
// the app bundle. Signing inner code before the outer bundle is the order Apple
// requires; --deep is avoided because it cannot apply per-file requirements.
const nested = [];
const collectDylibs = async (dir) => {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) await collectDylibs(full);
    else if (entry.isFile() && entry.name.endsWith(".dylib")) nested.push(full);
  }
};
await collectDylibs(app);
for (const file of nested) mustRun(`codesign ${file}`, "codesign", codesignArguments(identity.hash, file));
mustRun("codesign runtime", "codesign", codesignArguments(identity.hash, runtime));
mustRun("codesign app", "codesign", codesignArguments(identity.hash, app));
mustRun("verify signature", "codesign", ["--verify", "--deep", "--strict", "--verbose=2", app]);

const mustNotarize = async (archive) => {
  try {
    await notarize(signingConfig, archive, keyPath, notarizationDeadline);
  } catch (error) {
    console.error(error.message);
    process.exit(1);
  }
};

// Notarize both artifacts within the shared deadline. Only Accepted status permits stapling.
const submissionZip = join(workDir, "muniment-notarize.zip");
mustRun("zip for notarization", "ditto", ["-c", "-k", "--keepParent", app, submissionZip]);
await mustNotarize(submissionZip);

// Staple the ticket into the bundle so Gatekeeper validates offline, then
// verify the staple before packaging the release archive.
mustRun("staple", "xcrun", stapleArguments(app));
mustRun("validate staple", "xcrun", ["stapler", "validate", app]);

packageApp();
mustRun("make package directory", "mkdir", ["-p", pkgDir]);
mustRun("build signed installer", "productbuild",
  productbuildArguments(app, pkg, installerIdentity.hash, keychain));
await mustNotarize(pkg);
mustRun("staple installer", "xcrun", stapleArguments(pkg));
mustRun("validate installer staple", "xcrun", ["stapler", "validate", pkg]);
console.log(`kept ${appZip} (signed + notarized + stapled)`);
console.log(`kept ${pkg} (signed + notarized + stapled)`);
