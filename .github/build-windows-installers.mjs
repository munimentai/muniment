import { copyFile, readdir, rename } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { spawnSync } from "node:child_process";

const run = (...args) => {
  const cli = join("node_modules", "@tauri-apps", "cli", "tauri.js");
  const result = spawnSync(process.execPath, [cli, ...args], { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};

const msiDirectory = join("src-tauri", "target", "release", "bundle", "msi");
const soleMsi = async () => {
  const matches = (await readdir(msiDirectory)).filter((name) => name.endsWith(".msi"));
  if (matches.length !== 1) throw new Error(`expected one MSI in ${msiDirectory}, found: ${matches.join(", ") || "none"}`);
  return join(msiDirectory, matches[0]);
};

// Preserve the normal MSI while the second bundling pass writes the fleet variant.
run("build");
const userMsi = await soleMsi();
const savedUserMsi = join(dirname(userMsi), `.${basename(userMsi)}.per-user`);
await rename(userMsi, savedUserMsi);

// Keep an older product version for the Windows VM's major-upgrade verification.
// The current build remains the release artifact and is the only machine MSI uploaded.
const upgradeBaseMsi = join(dirname(msiDirectory), ".machine-upgrade-base.msi");
run(
  "build", "--bundles", "msi",
  "--config", "src-tauri/tauri.machine.conf.json",
  "--config", JSON.stringify({ version: "0.0.0" }),
);
await copyFile(await soleMsi(), upgradeBaseMsi);
run("build", "--bundles", "msi", "--config", "src-tauri/tauri.machine.conf.json");
const generatedMachineMsi = await soleMsi();
const machineMsi = generatedMachineMsi.replace(/\.msi$/, "-machine.msi");
await rename(generatedMachineMsi, machineMsi);
await rename(savedUserMsi, userMsi);

console.log(`kept ${userMsi}`);
console.log(`created ${machineMsi}`);
