import { readdir, rename } from "node:fs/promises";
import { existsSync, readFileSync, copyFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import {
  resolveSigningConfiguration,
  signArguments,
  signToolInstallArgs,
  tauriSignCommand,
} from "./lib/windows-signing.mjs";

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
// Signing config comes from the unit-tested windows-signing module: null when no
// AZURE_* creds are present (unsigned build), or a resolved config (throws if the
// set is partial). Microsoft's `sign` dotnet tool signs on its own (no signtool)
// and authenticates the service principal via Azure.Identity DefaultAzureCredential
// reading AZURE_TENANT_ID/CLIENT_ID/CLIENT_SECRET from the env (no az login).
const signingConfig = resolveSigningConfiguration(process.env);
const signing = signingConfig !== null;

let signArgs = [];
if (signing) {
  // Ensure the signing toolchain (.NET SDK + `sign`). Self-provisioning keeps the
  // build working even without the baked template; on template 9950 (which bakes
  // .NET + `sign`) these installs are fast no-ops.
  const onPath = (exe) => spawnSync("where", [exe], { encoding: "utf8" }).status === 0;
  if (!onPath("dotnet")) {
    console.log("installing .NET SDK (build-time; template 9950 bakes it to skip)...");
    const dotnetDir = join(tmpdir(), "dotnet");
    const r = spawnSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command",
      `Invoke-WebRequest -UseBasicParsing https://dot.net/v1/dotnet-install.ps1 -OutFile $env:TEMP\\di.ps1; `
      + `& $env:TEMP\\di.ps1 -Channel 8.0 -InstallDir '${dotnetDir}' -NoPath`], { stdio: "inherit" });
    if (r.status !== 0) process.exit(r.status ?? 1);
    process.env.DOTNET_ROOT = dotnetDir;
    process.env.PATH = `${dotnetDir};${process.env.PATH}`;
  }
  if (!onPath("sign")) {
    console.log("installing dotnet `sign` tool (build-time)...");
    const signDir = join(tmpdir(), "signtools");
    const r = spawnSync("dotnet", signToolInstallArgs(signDir), { stdio: "inherit" });
    if (r.status !== 0) process.exit(r.status ?? 1);
    process.env.PATH = `${signDir};${process.env.PATH}`;
  }

  // Tauri signs the app .exe (before packaging) and the NSIS installer with this
  // custom command; %1 is each file path.
  signArgs = ["--config", JSON.stringify({ bundle: { windows: { signCommand: tauriSignCommand(signingConfig) } } })];
  console.log("windows signing ENABLED (Azure Artifact Signing via dotnet `sign`)");

  // Preflight: confirm the toolchain and do ONE real test-sign on a throwaway
  // copy with output inherited, so the real `sign` error surfaces (tauri's
  // signCommand only reports "failed to run …") and we fail before the ~5-min
  // app compile.
  for (const [cmd, cmdArgs] of [["where", ["dotnet"]], ["dotnet", ["--version"]],
      ["where", ["sign"]]]) {
    const r = spawnSync(cmd, cmdArgs, { encoding: "utf8" });
    console.log(`preflight: ${cmd} ${cmdArgs.join(" ")} -> rc=${r.status} `
      + `${((r.stdout || "") + (r.stderr || "")).replace(/\s+/g, " ").trim()}`);
  }
  const probeTarget = join(tmpdir(), "signprobe.exe");
  copyFileSync(process.execPath, probeTarget);
  console.log("preflight: test-signing a throwaway copy to surface the real error...");
  const probe = spawnSync("sign", signArguments(signingConfig, probeTarget), { stdio: "inherit" });
  if (probe.status !== 0) {
    console.error(`signing preflight FAILED (sign rc=${probe.status}) — see output above`);
    process.exit(probe.status ?? 1);
  }
  console.log("signing preflight OK");
} else {
  console.log("windows signing SKIPPED: Azure credentials absent (unsigned build)");
}

const signFile = (file) => {
  if (!signing) return;
  const result = spawnSync("sign", signArguments(signingConfig, file), { stdio: "inherit" });
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
