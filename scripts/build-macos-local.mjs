// Build, sign and install the macOS desktop app for the local proof.
// The bundler signs nothing; this script signs every Mach-O leaf first, then
// the runtime, then the app, with one identity, the order the nightly uses in
// .github/build-macos-app.mjs. One identity on every Mach-O is what dyld and
// launchd check, and no hand codesign follows.
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { readdir } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const identity = process.env.MUNIMENT_SIGNING_IDENTITY ?? "Muniment Local";
// The local identity carries no Team ID, and the hardened runtime's library
// validation admits only a dylib from the same Team ID, so the local build
// signs without the runtime option. The nightly's Developer ID has a team and
// keeps it. No timestamp: a self-signed signature has nothing to timestamp.
const codesignArguments = (identityName, file) => ["--force", "--sign", identityName, file];
const target = join("src-tauri", "target");
const runtimeSource = join(target, "aarch64-apple-darwin", "release", "muniment-runtime");
const runtimeBundled = join(target, "universal-apple-darwin", "release", "muniment-runtime");
const app = join(target, "release", "bundle", "macos", "muniment.app");
const runtime = join(app, "Contents", "Library", "LaunchServices", "muniment-runtime");
const home = homedir();
const installed = join(home, "Applications", "muniment.app");

const mustRun = (label, cmd, args, options = {}) => {
  const result = spawnSync(cmd, args, { stdio: "inherit", ...options });
  if (result.error) throw result.error;
  if (result.status !== 0 && !options.allowFailure) {
    console.error(`${label} FAILED (${cmd} rc=${result.status})`);
    process.exit(result.status ?? 1);
  }
};

// The bundle config reads the runtime from the universal path; a local build is arm64 only.
mustRun("build runtime", "cargo", ["build", "--manifest-path", "src-tauri/Cargo.toml", "--package", "muniment-runtime", "--release", "--locked", "--target", "aarch64-apple-darwin"]);
mkdirSync(join(target, "universal-apple-darwin", "release"), { recursive: true });
cpSync(runtimeSource, runtimeBundled);

mustRun("build app", process.execPath, [join("node_modules", "@tauri-apps", "cli", "tauri.js"), "build", "--bundles", "app", "--no-sign"]);

const nested = [];
const collectDylibs = async (dir) => {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = join(dir, entry.name);
    if (entry.isDirectory()) await collectDylibs(full);
    else if (entry.isFile() && entry.name.endsWith(".dylib")) nested.push(full);
  }
};
await collectDylibs(app);
for (const file of nested) mustRun(`codesign ${file}`, "codesign", codesignArguments(identity, file));
mustRun("codesign runtime", "codesign", codesignArguments(identity, runtime));
mustRun("codesign app", "codesign", codesignArguments(identity, app));
mustRun("verify signature", "codesign", ["--verify", "--deep", "--strict", "--verbose=2", app]);

// Stop the old build and clear what it left, so the proof is clean.
mustRun("stop app", "pkill", ["-x", "muniment-desktop"], { allowFailure: true });
mustRun("stop runtime", "pkill", ["-f", "LaunchServices/muniment-runtime"], { allowFailure: true });
for (const path of [
  join(home, "Library", "Caches", "ai.muniment.desktop"),
  join(home, "Library", "WebKit", "ai.muniment.desktop"),
  join(home, "Library", "Preferences", "ai.muniment.desktop.plist"),
  join(home, "Library", "Saved Application State", "ai.muniment.desktop.savedState"),
  installed,
]) rmSync(path, { recursive: true, force: true });
if (!existsSync(join(home, "Applications"))) mkdirSync(join(home, "Applications"));
mustRun("install app", "cp", ["-R", app, installed]);
mustRun("verify installed signature", "codesign", ["--verify", "--deep", "--strict", installed]);
console.log(`installed ${installed} signed as ${identity}`);
