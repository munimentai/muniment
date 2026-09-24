import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { readdir } from "node:fs/promises";
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
} from "./lib/macos-signing.mjs";
import { readSigningEnvironment } from "./lib/signing-env.mjs";

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
const cliBinary = join(app, "Contents", "Library", "LaunchServices", "muniment-cli");
const readerBinary = join(app, "Contents", "Library", "LaunchServices", "muniment-reader");
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

// Apple Developer ID credentials arrive on stdin: the nightly's first step
// takes the signing line out of dci_env (see lib/signing-env.mjs) and pipes it
// here, so the dependency and compile steps before this process never see it.
// This process reads stdin to its end before it starts a child, and keeps the
// credentials out of process.env, so the build children below inherit neither.
// macOS denies a same-user process access to this process's memory. The
// config flag controls signing.
const signing = signingEnabled(process.env);
const signingConfig = signing ? resolveSigningConfiguration(readSigningEnvironment()) : null;
if (signing && signingConfig === null) {
  throw new Error("MACOS_SIGNING_ENABLED is true but Apple credentials are absent");
}

// Build the universal app without Tauri signing so this script checks trust before the first codesign call.
mustRun("build universal runtime", process.execPath, [join(".github", "build-macos-runtime.mjs")]);
tauri("build", "--target", "universal-apple-darwin", "--no-bundle", "--no-sign");
mustRun("build universal CEF helper", process.execPath, [join(".github", "build-macos-cef-helper.mjs")]);
tauri("bundle", "--target", "universal-apple-darwin", "--bundles", "app", "--no-sign");
mustRun("package universal CEF", process.execPath, ["scripts/package-cef-macos.mjs", app, "--universal"]);

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
// The signing tools resolve from the system directories alone, so nothing a
// build step wrote into a user-owned PATH entry runs with the credentials.
process.env.PATH = "/usr/bin:/bin:/usr/sbin:/sbin";

// Everything secret-bearing lives in a throwaway directory removed on exit.
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
// CEF contains extensionless Mach-O code and nested helper apps as well as dylibs.
const cefFrameworks = join(app, "Contents", "Frameworks");
mustRun("codesign CEF framework", "codesign", codesignArguments(identity.hash, join(cefFrameworks, "Chromium Embedded Framework.framework")));
for (const suffix of ["", " (GPU)", " (Renderer)", " (Plugin)", " (Alerts)"]) {
  const helperApp = join(cefFrameworks, `muniment CEF Helper${suffix}.app`);
  mustRun("codesign nested CEF helper", "codesign", [...codesignArguments(identity.hash, helperApp), "--entitlements", "src-tauri/packaging/entitlements.plist"]);
}
mustRun("codesign runtime", "codesign", codesignArguments(identity.hash, runtime));
mustRun("codesign cli", "codesign", codesignArguments(identity.hash, cliBinary));
mustRun("codesign reader", "codesign", codesignArguments(identity.hash, readerBinary));
mustRun("codesign CEF helper", "codesign", [...codesignArguments(identity.hash, join(app, "Contents", "MacOS", "muniment-cef-helper")), "--entitlements", "src-tauri/packaging/entitlements.plist"]);
mustRun("codesign app", "codesign", [...codesignArguments(identity.hash, app), "--entitlements", "src-tauri/packaging/entitlements.plist"]);
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
