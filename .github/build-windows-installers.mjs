import { readdir, rename } from "node:fs/promises";
import { existsSync, readFileSync, copyFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { tmpdir } from "node:os";
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

// Azure Artifact Signing credentials arrive as a file the desktop-ci driver
// writes into the VM over the SSH data channel (never on argv). Load them so
// tauri's signCommand and the explicit MSI signing below can authenticate. When
// the file or any variable is absent the build proceeds UNSIGNED — the nightly
// keeps shipping and signing turns on the moment the six repo secrets exist.
const credFile = join(tmpdir(), "dci_env");
if (existsSync(credFile)) {
  for (const line of readFileSync(credFile, "utf8").split(/\r?\n/)) {
    const eq = line.indexOf("=");
    if (eq > 0) process.env[line.slice(0, eq).trim()] = line.slice(eq + 1);
  }
}
const SIGN_VARS = ["AZURE_TENANT_ID", "AZURE_CLIENT_ID", "AZURE_CLIENT_SECRET",
  "AZURE_SIGNING_ENDPOINT", "AZURE_SIGNING_ACCOUNT", "AZURE_SIGNING_PROFILE"];
const signing = SIGN_VARS.every((v) => process.env[v]);

let signArgs = [];
if (signing) {
  // artifact-signing-cli is the renamed trusted-signing-cli; it wraps signtool
  // (Windows SDK, on the CI template) and cargo is already present, so no extra
  // toolchain is provisioned.
  const install = spawnSync("cargo", ["install", "artifact-signing-cli"], { stdio: "inherit" });
  if (install.status !== 0) process.exit(install.status ?? 1);
  const signCommand = `artifact-signing-cli -e ${process.env.AZURE_SIGNING_ENDPOINT}`
    + ` -a ${process.env.AZURE_SIGNING_ACCOUNT} -c ${process.env.AZURE_SIGNING_PROFILE} %1`;
  signArgs = ["--config", JSON.stringify({ bundle: { windows: { signCommand } } })];
  console.log("windows signing ENABLED (Azure Artifact Signing)");

  // Preflight: probe the signing toolchain, then do ONE real signing call on a
  // throwaway copy with output inherited, so the actual artifact-signing-cli
  // error surfaces (tauri's signCommand reports only "failed to run …") and we
  // fail before the ~5-min app compile. artifact-signing-cli needs signtool from
  // Windows 11 SDK 10.0.26100+ and authenticates via the AZURE_* env vars.
  for (const [cmd, cmdArgs] of [["where", ["signtool"]], ["where", ["dotnet"]],
      ["dotnet", ["--version"]], ["where", ["az"]], ["where", ["artifact-signing-cli"]]]) {
    const r = spawnSync(cmd, cmdArgs, { encoding: "utf8" });
    console.log(`preflight: ${cmd} ${cmdArgs.join(" ")} -> rc=${r.status} `
      + `${((r.stdout || "") + (r.stderr || "")).replace(/\s+/g, " ").trim()}`);
  }
  const probeTarget = join(tmpdir(), "signprobe.exe");
  copyFileSync(process.execPath, probeTarget);
  console.log("preflight: test-signing a throwaway copy to surface the real error...");
  const probe = spawnSync("artifact-signing-cli",
    ["-e", process.env.AZURE_SIGNING_ENDPOINT, "-a", process.env.AZURE_SIGNING_ACCOUNT,
     "-c", process.env.AZURE_SIGNING_PROFILE, probeTarget], { stdio: "inherit" });
  if (probe.status !== 0) {
    console.error(`signing preflight FAILED (artifact-signing-cli rc=${probe.status}) — see output above`);
    process.exit(probe.status ?? 1);
  }
  console.log("signing preflight OK");
} else {
  console.log("windows signing SKIPPED: Azure credentials absent (unsigned build)");
}

const signFile = (file) => {
  if (!signing) return;
  const result = spawnSync("artifact-signing-cli",
    ["-e", process.env.AZURE_SIGNING_ENDPOINT, "-a", process.env.AZURE_SIGNING_ACCOUNT,
     "-c", process.env.AZURE_SIGNING_PROFILE, file], { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};

// Preserve the normal MSI while the second bundling pass writes the fleet variant.
// signArgs makes tauri sign the app .exe (before packaging) and the NSIS installer.
run("build", ...signArgs);
const userMsi = await soleMsi();
const savedUserMsi = join(dirname(userMsi), `.${basename(userMsi)}.per-user`);
await rename(userMsi, savedUserMsi);

// Keep a distinct build for the Windows VM's same-version major-upgrade verification.
// The WiX template enables AllowSameVersionUpgrades; the current build remains the
// release artifact and is the only machine MSI uploaded.
const upgradeBaseMsi = join(dirname(msiDirectory), "machine-upgrade-base.msi");
// The upgrade-base is a throwaway fixture for the in-place-upgrade test — unsigned.
run("build", "--bundles", "msi", "--config", "src-tauri/tauri.machine.conf.json");
await rename(await soleMsi(), upgradeBaseMsi);
run("build", "--bundles", "msi", "--config", "src-tauri/tauri.machine.conf.json", ...signArgs);
const generatedMachineMsi = await soleMsi();
const machineMsi = generatedMachineMsi.replace(/\.msi$/, "-machine.msi");
await rename(generatedMachineMsi, machineMsi);
await rename(savedUserMsi, userMsi);

// tauri's signCommand covers the app .exe and the NSIS installer, but not the
// MSI package itself — sign both shipped MSIs explicitly (no-op when unsigned).
signFile(userMsi);
signFile(machineMsi);

console.log(`kept ${userMsi}`);
console.log(`created ${machineMsi}`);
