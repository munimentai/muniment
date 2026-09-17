import { existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const targetDir = join("src-tauri", "target");
const runtime = join(targetDir, "universal-apple-darwin", "release", "muniment-runtime");
const cli = join(targetDir, "universal-apple-darwin", "release", "muniment-cli");
const reader = join(targetDir, "universal-apple-darwin", "release", "muniment-reader");

const mustRun = (cmd, args) => {
  const result = spawnSync(cmd, args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};
const mustRunWithEnv = (cmd, args, options) => {
  const result = spawnSync(cmd, args, { stdio: "inherit", ...options });
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

// The reader sidecar is Go. Each slice builds with CGO off, so the binary
// carries no dylib and signs like the Rust ones.
for (const [target, arch] of [["x86_64-apple-darwin", "amd64"], ["aarch64-apple-darwin", "arm64"]]) {
  mkdirSync(join(targetDir, target, "release"), { recursive: true });
  mustRunWithEnv("bash", [join(".github", "build-reader.sh"), join(targetDir, target, "release", "muniment-reader")], {
    env: { ...process.env, GOOS: "darwin", GOARCH: arch },
  });
}

mkdirSync(join(targetDir, "universal-apple-darwin", "release"), { recursive: true });
for (const [name, output] of [["muniment-runtime", runtime], ["muniment-cli", cli], ["muniment-reader", reader]]) {
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
