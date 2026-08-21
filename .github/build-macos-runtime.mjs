import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const targetDir = join("src-tauri", "target");
const runtime = join(targetDir, "universal-apple-darwin", "release", "muniment-runtime");

const mustRun = (cmd, args) => {
  const result = spawnSync(cmd, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};

for (const target of ["x86_64-apple-darwin", "aarch64-apple-darwin"]) {
  mustRun("cargo", [
    "build", "--manifest-path", "src-tauri/Cargo.toml", "--package", "muniment-runtime",
    "--release", "--locked", "--target", target,
  ]);
}

mkdirSync(join(targetDir, "universal-apple-darwin", "release"), { recursive: true });
mustRun("lipo", [
  "-create",
  join(targetDir, "x86_64-apple-darwin", "release", "muniment-runtime"),
  join(targetDir, "aarch64-apple-darwin", "release", "muniment-runtime"),
  "-output", runtime,
]);

if (!existsSync(runtime)) {
  throw new Error(`macOS runtime source is absent: ${runtime}`);
}
