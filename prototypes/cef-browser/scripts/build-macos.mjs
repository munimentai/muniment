import { ensureCli } from './tauri-cli.mjs';
import { openSync, readSync, closeSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { readdir } from "node:fs/promises";
import { randomBytes } from "node:crypto";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import {
  codesignArguments,
  intermediateCertificateImportArguments,
  keychainSearchListArguments,
  signingCertificateImportArguments,
  signingIdentityArguments,
  signingKeyPartitionListArguments,
} from "../../../.github/lib/macos-signing.mjs";

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

import { fileURLToPath } from "node:url";
const prototype = fileURLToPath(new URL("..", import.meta.url));
const repo = fileURLToPath(new URL("../../..", import.meta.url));
process.chdir(repo);
const app = join(prototype, "src-tauri", "target", "release", "bundle", "macos", "Muniment Browser Prototype.app");
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

mustRun("build prototype", ensureCli(), ["tauri", "build", "--bundles", "app"], { cwd: prototype });
// CEF alone links through the compatibility library. It reexports all other
// Security APIs unchanged. Apply before signing, never mutate a signed install.
const cefFramework = join(app, "Contents", "Frameworks", "Chromium Embedded Framework.framework", "Chromium Embedded Framework");
mustRun("link CEF private Keychain bridge", "install_name_tool", [
  "-change", "/System/Library/Frameworks/Security.framework/Versions/A/Security",
  "@loader_path/../libmuniment_cef_keychain.dylib", cefFramework,
]);
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
  mustRun("sign CEF component", "codesign", [...codesignArguments(identity[1], path), "--preserve-metadata=entitlements"]);
}
mustRun("verify CEF bundle", "codesign", ["--verify", "--deep", "--strict", app]);
console.log(`Signed prototype: ${app}`);
