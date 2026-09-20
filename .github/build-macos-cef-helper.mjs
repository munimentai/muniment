import { spawnSync } from "node:child_process";
import { join } from "node:path";

// Tauri combines only the main executable for a universal build.
const target = join("src-tauri", "target");
const binary = "muniment-cef-helper";
const result = spawnSync("lipo", [
  "-create",
  join(target, "aarch64-apple-darwin", "release", binary),
  join(target, "x86_64-apple-darwin", "release", binary),
  "-output", join(target, "universal-apple-darwin", "release", binary),
], { stdio: "inherit" });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
