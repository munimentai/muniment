import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const targetDir = join("src-tauri", "target");
const runtime = join(targetDir, "universal-apple-darwin", "release", "muniment-runtime");
const cli = join(targetDir, "universal-apple-darwin", "release", "muniment-cli");

const mustRun = (cmd, args) => {
  const result = spawnSync(cmd, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};

// The record server, muniment-cli, ships beside the runtime and is built the same way.
for (const target of ["x86_64-apple-darwin", "aarch64-apple-darwin"]) {
  mustRun("cargo", [
    "build", "--manifest-path", "src-tauri/Cargo.toml", "--package", "muniment-runtime",
    "--package", "muniment-cli", "--release", "--locked", "--target", target,
  ]);
}

mkdirSync(join(targetDir, "universal-apple-darwin", "release"), { recursive: true });
for (const [name, output] of [["muniment-runtime", runtime], ["muniment-cli", cli]]) {
  mustRun("lipo", [
    "-create",
    join(targetDir, "x86_64-apple-darwin", "release", name),
    join(targetDir, "aarch64-apple-darwin", "release", name),
    "-output", output,
  ]);
  if (!existsSync(output)) {
    throw new Error(`macOS ${name} source is absent: ${output}`);
  }
}
