import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { readdir } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import {
  codesignArguments,
  notarytoolSubmitArguments,
  parseSigningIdentity,
  resolveSigningConfiguration,
  stapleArguments,
} from "./lib/macos-signing.mjs";

const bundleDir = join("src-tauri", "target", "universal-apple-darwin", "release", "bundle", "macos");
const app = join(bundleDir, "muniment.app");
const appZip = `${app}.zip`;

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
// When the file or any variable is absent the build proceeds UNSIGNED — the
// nightly keeps shipping and signing turns on the moment the six repo secrets
// exist (enrollment Y5DUNHQA74 is still in review).
const credFile = join(tmpdir(), "dci_env");
if (existsSync(credFile)) {
  for (const line of readFileSync(credFile, "utf8").split(/\r?\n/)) {
    const eq = line.indexOf("=");
    if (eq > 0) process.env[line.slice(0, eq).trim()] = line.slice(eq + 1);
  }
}
// null when no APPLE_* creds are present (unsigned build), or a resolved config
// (throws if the set is partial), from the unit-tested macos-signing module.
const signingConfig = resolveSigningConfiguration(process.env);
const signing = signingConfig !== null;

// Always build the universal .app first; signing (when enabled) operates on the
// finished bundle so the unsigned and signed paths build identical bits.
tauri("build", "--target", "universal-apple-darwin", "--bundles", "app");

if (!signing) {
  console.log("macOS signing SKIPPED: Apple credentials absent (unsigned build)");
  packageApp();
  console.log(`kept ${appZip} (unsigned)`);
  process.exit(0);
}

console.log("macOS signing ENABLED (Developer ID codesign + notarytool notarization + staple)");

// Everything secret-bearing lives in a throwaway directory removed on exit.
const workDir = mkdtempSync(join(tmpdir(), "muniment-macos-signing-"));
const certPath = join(workDir, "certificate.p12");
const keyPath = join(workDir, "AuthKey.p8");
const keychain = join(workDir, "muniment-signing.keychain-db");
const keychainPassword = "muniment-ci-signing";
const cleanup = () => rmSync(workDir, { recursive: true, force: true });
process.on("exit", cleanup);

writeFileSync(certPath, Buffer.from(signingConfig.certificate, "base64"), { mode: 0o600 });
writeFileSync(keyPath, Buffer.from(signingConfig.apiKey, "base64"), { mode: 0o600 });

// A dedicated keychain holds ONLY the Developer ID cert, so identity discovery
// is unambiguous and the login keychain is never touched. Prepend it to the
// search list (keeping the existing entries) so codesign can find the key.
mustRun("create keychain", "security", ["create-keychain", "-p", keychainPassword, keychain]);
mustRun("keychain settings", "security", ["set-keychain-settings", keychain]);
mustRun("unlock keychain", "security", ["unlock-keychain", "-p", keychainPassword, keychain]);
mustRun("import certificate", "security",
  ["import", certPath, "-k", keychain, "-P", signingConfig.certificatePassword, "-T", "/usr/bin/codesign"]);
mustRun("authorize codesign", "security",
  ["set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s", "-k", keychainPassword, keychain]);
const priorKeychains = (spawnSync("security", ["list-keychains", "-d", "user"], { encoding: "utf8" }).stdout || "")
  .split(/\r?\n/).map((line) => line.trim().replace(/^"|"$/g, "")).filter(Boolean);
mustRun("register keychain", "security",
  ["list-keychains", "-d", "user", "-s", keychain, ...priorKeychains]);

const found = spawnSync("security", ["find-identity", "-v", "-p", "codesigning", keychain], { encoding: "utf8" });
const identity = parseSigningIdentity(found.stdout || "");
if (!identity) {
  console.error("no Developer ID Application identity found in the imported certificate");
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
mustRun("codesign app", "codesign", codesignArguments(identity.hash, app));
mustRun("verify signature", "codesign", ["--verify", "--deep", "--strict", "--verbose=2", app]);

// Notarize the signed bundle. notarytool submits a zip; --wait blocks until
// Apple accepts or rejects, so a rejected build fails here instead of shipping.
const submissionZip = join(workDir, "muniment-notarize.zip");
mustRun("zip for notarization", "ditto", ["-c", "-k", "--keepParent", app, submissionZip]);
mustRun("notarize", "xcrun", notarytoolSubmitArguments(signingConfig, submissionZip, keyPath));

// Staple the ticket into the bundle so Gatekeeper validates offline, then
// verify the staple before packaging the release archive.
mustRun("staple", "xcrun", stapleArguments(app));
mustRun("validate staple", "xcrun", ["stapler", "validate", app]);

packageApp();
console.log(`kept ${appZip} (signed + notarized + stapled)`);
