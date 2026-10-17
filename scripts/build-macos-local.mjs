// Build, sign and install the macOS desktop app for the local proof.
// Direct CEF uses the v2 shell. The Apple signing certificate comes from the
// environment at sign time into a throwaway keychain, every Mach-O is signed leaf
// first, then the runtime, then the app, with the nightly's codesign arguments
// from .github/build-macos-app.mjs, and the keychain is deleted on exit.
// Nothing here stores the certificate, and no hand codesign follows.
import { cpSync, openSync, readSync, closeSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, renameSync, writeFileSync } from "node:fs";
import { readdir } from "node:fs/promises";
import { randomBytes } from "node:crypto";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { nativeFiles } from "../.github/lib/macos-variants.mjs";
import {
  codesignArguments,
  intermediateCertificateImportArguments,
  keychainSearchListArguments,
  signingCertificateImportArguments,
  signingIdentityArguments,
  signingKeyPartitionListArguments,
} from "../.github/lib/macos-signing.mjs";

const home = homedir();
// The signing certificate comes from the environment, so no host, path or
// secret store sits in the repository:
//   MUNIMENT_SIGNING_CERTIFICATE_FILE      path to the Developer ID .p12 file
//   MUNIMENT_SIGNING_CERTIFICATE           base64 of that .p12, used when no file is named
//   MUNIMENT_SIGNING_CERTIFICATE_PASSWORD  the .p12 password
const requireEnv = (name, meaning) => {
  const value = process.env[name];
  if (!value) {
    console.error(`Set ${name} to ${meaning}.`);
    process.exit(1);
  }
  return value;
};
const certificateFile = process.env.MUNIMENT_SIGNING_CERTIFICATE_FILE;
if (certificateFile && !existsSync(certificateFile)) {
  console.error(`MUNIMENT_SIGNING_CERTIFICATE_FILE names ${certificateFile}, which does not exist.`);
  process.exit(1);
}
const certificateBase64 = certificateFile ? undefined : requireEnv("MUNIMENT_SIGNING_CERTIFICATE", "the base64 Developer ID .p12, or set MUNIMENT_SIGNING_CERTIFICATE_FILE to its path");
const certificatePassword = requireEnv("MUNIMENT_SIGNING_CERTIFICATE_PASSWORD", "the .p12 password");

const target = join("src-tauri", "target");
const runtimeSource = join(target, "aarch64-apple-darwin", "release", "muniment-runtime");
const runtimeBundled = join(target, "universal-apple-darwin", "release", "muniment-runtime");
const cliSource = join(target, "aarch64-apple-darwin", "release", "muniment-cli");
const cliBundled = join(target, "universal-apple-darwin", "release", "muniment-cli");
const readerSource = join(target, "aarch64-apple-darwin", "release", "muniment-reader");
const readerBundled = join(target, "universal-apple-darwin", "release", "muniment-reader");
const app = join(target, "release", "bundle", "macos", "muniment.app");
const runtime = join(app, "Contents", "Library", "LaunchServices", "muniment-runtime");
const readerBinary = join(app, "Contents", "Library", "LaunchServices", "muniment-reader");
const installed = join(home, "Applications", "muniment.app");

const mustRun = (label, cmd, args, options = {}) => {
  const result = spawnSync(cmd, args, { stdio: "inherit", ...options });
  if (result.error) throw result.error;
  if (result.status !== 0 && !options.allowFailure) {
    console.error(`${label} FAILED (${cmd} rc=${result.status})`);
    process.exit(result.status ?? 1);
  }
};
// Capture stdout for a command whose output must never reach the terminal.
const capture = (label, cmd, args, options = {}) => {
  const result = spawnSync(cmd, args, { encoding: "utf8", stdio: ["pipe", "pipe", "inherit"], ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    console.error(`${label} FAILED (${cmd} rc=${result.status})`);
    process.exit(result.status ?? 1);
  }
  return result.stdout;
};

// The candidate Pi track is the one path: the switch is read at compile time by both crates.
process.env.MUNIMENT_PI_CANDIDATE = "1";
// Build first, so a build failure never touches the credential.
// The bundle config reads the runtime from the universal path; a local build is arm64 only.
mustRun("build runtime", "cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--package", "muniment-runtime", "--package", "muniment-cli", "--release", "--locked", "--target", "aarch64-apple-darwin"]);
// The reader sidecar is Go with CGO off, so it signs like the Rust binaries.
mustRun("build reader", "bash", [join(".github", "build-reader.sh"), readerSource], {
  env: { ...process.env, GOOS: "darwin", GOARCH: "arm64" },
});
mkdirSync(join(target, "universal-apple-darwin", "release"), { recursive: true });
cpSync(runtimeSource, runtimeBundled);
cpSync(cliSource, cliBundled);
cpSync(readerSource, readerBundled);
mustRun("build browser helper", "cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--bin", "muniment-cef-helper", "--release", "--locked"]);
mustRun("build app", "npm", ["run", "tauri", "build", "--", "--bundles", "app", "--features", "local-runtime", "--no-sign"]);
mustRun("package direct CEF", process.execPath, ["scripts/package-cef-macos.mjs", app]);
// A local build is arm64 only. The vendored speech libraries are universal.
for (const file of nativeFiles(app)) {
  if (capture("read architectures", "lipo", ["-archs", file]).trim().split(/\s+/).length > 1) mustRun("thin native code", "lipo", [file, "-thin", "arm64", "-output", file]);
}

// Everything secret-bearing lives in a throwaway directory removed on exit.
const workDir = mkdtempSync(join(tmpdir(), "muniment-local-signing-"));
const keychain = join(workDir, "muniment-local-signing.keychain-db");
const keychainPassword = randomBytes(24).toString("hex");
const certificatePath = join(workDir, "certificate.p12");
const priorKeychains = capture("read keychain search list", "security", ["list-keychains", "-d", "user"]);
let keychainCreated = false;
// trustd builds the signer's chain from the login keychain, not from a keychain
// the run adds to the search list, so the public Developer ID G2 intermediate
// sits in the login keychain for the run and leaves with it. The identity never does.
const loginKeychain = join(home, "Library", "Keychains", "login.keychain-db");
const intermediateCertificate = join(".github", "certs", "DeveloperIDG2CA.cer");
const intermediateSha1 = capture("read intermediate fingerprint", "openssl", ["x509", "-inform", "der", "-in", intermediateCertificate, "-noout", "-fingerprint", "-sha1"]).replace(/^.*=/s, "").replace(/:/g, "").trim();
let intermediatePlaced = false;
process.on("exit", () => {
  if (keychainCreated) {
    spawnSync("security", keychainSearchListArguments("", priorKeychains).filter((arg) => arg !== ""));
    spawnSync("security", ["delete-keychain", keychain]);
  }
  if (intermediatePlaced) spawnSync("security", ["delete-certificate", "-Z", intermediateSha1, loginKeychain]);
  rmSync(workDir, { recursive: true, force: true });
});

// A PKCS#12 file is a DER sequence, so its first byte is 0x30.
const certificate = certificateFile ? readFileSync(certificateFile) : Buffer.from(certificateBase64.trim(), "base64");
if (certificate[0] !== 0x30) {
  console.error(certificateFile ? `MUNIMENT_SIGNING_CERTIFICATE_FILE ${certificateFile} is not a .p12 file` : "MUNIMENT_SIGNING_CERTIFICATE is not the base64 of a .p12 file");
  process.exit(1);
}
writeFileSync(certificatePath, certificate, { mode: 0o600 });

// The throwaway keychain holds the identity for this run alone.
mustRun("create keychain", "security", ["create-keychain", "-p", keychainPassword, keychain]);
keychainCreated = true;
mustRun("keychain settings", "security", ["set-keychain-settings", keychain]);
mustRun("unlock keychain", "security", ["unlock-keychain", "-p", keychainPassword, keychain]);
mustRun("import certificate", "security", signingCertificateImportArguments(certificatePath, keychain, certificatePassword), { stdio: ["ignore", "ignore", "inherit"] });
mustRun("authorize codesign", "security", signingKeyPartitionListArguments(keychain, keychainPassword), { stdio: ["ignore", "ignore", "inherit"] });
const intermediatePresent = spawnSync("security", ["find-certificate", "-Z", "-c", "Developer ID Certification Authority", loginKeychain], { encoding: "utf8" });
if (intermediatePresent.error) throw intermediatePresent.error;
if (!(intermediatePresent.stdout || "").includes(intermediateSha1)) {
  mustRun("place intermediate", "security", intermediateCertificateImportArguments(intermediateCertificate, loginKeychain), { stdio: ["ignore", "ignore", "inherit"] });
  intermediatePlaced = true;
}
mustRun("register keychain", "security", keychainSearchListArguments(keychain, priorKeychains));
const identities = capture("list identities", "security", signingIdentityArguments(keychain));
const identity = identities.match(/^\s*\d+\) ([0-9A-F]{40})[ \t]+"((?:Developer ID Application|Apple Development|Apple Distribution):[^"\r\n]+)"/m);
if (!identity) {
  console.error("The certificate holds no Developer ID Application or Apple Development identity");
  process.exit(1);
}
console.log(`signing identity: ${identity[2]}`);

const binaries = [];
const bundles = [];
const scan = async (dir) => {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) {
      await scan(full);
      if (/\.(app|framework)$/.test(entry.name)) bundles.push(full);
    } else if (entry.isFile()) {
      const fd = openSync(full, "r");
      const magic = Buffer.alloc(4);
      try { readSync(fd, magic, 0, 4, 0); } finally { closeSync(fd); }
      if (["cffaedfe", "feedfacf", "cefaedfe", "feedface", "cafebabe", "bebafeca"].includes(magic.toString("hex"))) binaries.push(full);
    }
  }
};
await scan(app);
for (const path of [...binaries, ...bundles, app]) {
  mustRun("sign CEF component", "codesign", [...codesignArguments(identity[1], path), "--entitlements", "src-tauri/packaging/entitlements.plist"]);
}
mustRun("verify signature", "codesign", ["--verify", "--deep", "--strict", "--verbose=2", app]);

// A running service holds the old executable after a bundle replacement.
// Restart it after the swap so launchd reads the installed bundle.
if (!process.argv.includes("--build-only")) {
  mustRun("stop app", "pkill", ["-x", "muniment-desktop"], { allowFailure: true });
  const stopDeadline = Date.now() + 15000;
  while (spawnSync("pgrep", ["-x", "muniment-desktop"], { stdio: "ignore" }).status === 0) {
    if (Date.now() >= stopDeadline) throw new Error("The app did not close. The installed bundle was not replaced.");
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  mkdirSync(join(home, "Applications"), { recursive: true });
  const stage = mkdtempSync(join(home, "Applications", ".muniment-update-"));
  const stagedApp = join(stage, "muniment.app");
  mustRun("stage app", "ditto", [app, stagedApp]);
  mustRun("verify staged signature", "codesign", ["--verify", "--deep", "--strict", stagedApp]);
  const backup = mkdtempSync(join(tmpdir(), "muniment-app-backup-"));
  const hadApp = existsSync(installed);
  if (hadApp) renameSync(installed, join(backup, "muniment.app"));
  try { renameSync(stagedApp, installed); }
  catch (error) {
    if (hadApp) renameSync(join(backup, "muniment.app"), installed);
    throw error;
  }
  rmSync(stage, { recursive: true, force: true });
  mustRun("restart runtime", "pkill", ["-f", "LaunchServices/muniment-runtime"], { allowFailure: true });
  mustRun("verify installed signature", "codesign", ["--verify", "--deep", "--strict", installed]);
  console.log(`installed ${installed} signed as ${identity[2]}`);
} else {
  console.log(`signed build ready: ${app}`);
}
