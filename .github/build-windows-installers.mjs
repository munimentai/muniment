import { readdir, rename } from "node:fs/promises";
import { existsSync, copyFileSync, mkdirSync, rmSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import { takeSigningEnvironment } from "./lib/signing-env.mjs";
import {
  DOTNET_SDK,
  resolveSigningConfiguration,
  signArguments,
  signToolInstallArgs,
  tauriSignCommand,
} from "./lib/windows-signing.mjs";

// Azure Artifact Signing credentials arrive as the one signing line the
// desktop-ci preamble skips (see lib/signing-env.mjs). Take it out of the file
// before anything else runs, and hold it in this process alone. Dependency
// installation, compilation and the unsigned bundling pass run with an
// environment that carries no signing secret. Windows lets a process of the
// same user read another's memory, so this keeps the secret out of the
// environment and the disk, not out of reach of a hostile build step. When the
// line is absent the build proceeds UNSIGNED.
const signingEnvironment = takeSigningEnvironment();
const signingConfig = resolveSigningConfiguration(signingEnvironment);
const signing = signingConfig !== null;
// Every child starts with this environment. The signing phase below widens it.
let phaseEnvironment = { ...process.env };

// Installer jobs use fresh VMs, independently of the compile preflight.
if (process.platform === "win32") {
  const tools = spawnSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command",
    ". ./scripts/prepare-cef-windows.ps1; [Console]::Write($env:PATH)",
  ], { encoding: "utf8" });
  if (tools.error) throw tools.error;
  if (tools.status !== 0 || !tools.stdout.trim()) {
    process.stderr.write(tools.stderr || "CEF build tool setup failed.\n");
    process.exit(tools.status || 1);
  }
  process.env.PATH = tools.stdout.trim();
  phaseEnvironment.PATH = process.env.PATH;
}

// Release VMs install dependencies here, after the signing line is gone, and
// without package install scripts: the build needs none of them.
if (process.argv.includes("--install-dependencies")) {
  const install = spawnSync("npm ci --ignore-scripts --no-audit --no-fund", { stdio: "inherit", env: phaseEnvironment, shell: true });
  if (install.error) throw install.error;
  if (install.status !== 0) process.exit(install.status ?? 1);
}

const run = (command, pass, ...args) => {
  const cli = join("node_modules", "@tauri-apps", "cli", "tauri.js");
  console.log(`Starting the ${pass} bundling pass.`);
  const result = spawnSync(process.execPath, [cli, command, ...args], { stdio: "inherit", env: phaseEnvironment });
  const status = result.status ?? 1;
  if (result.error) {
    console.error(`The ${pass} bundling pass failed with exit status ${status}.`);
    throw result.error;
  }
  if (result.status !== 0) {
    console.error(`The ${pass} bundling pass failed with exit status ${status}.`);
    process.exit(status);
  }
};

const msiDirectory = join("src-tauri", "target", "release", "bundle", "msi");
const soleMsi = async () => {
  const matches = (await readdir(msiDirectory)).filter((name) => name.endsWith(".msi"));
  if (matches.length !== 1) throw new Error(`expected one MSI in ${msiDirectory}, found: ${matches.join(", ") || "none"}`);
  return join(msiDirectory, matches[0]);
};

let signArgs = [];
if (signing) {
  // Provision the signing toolchain (.NET SDK + `sign`) now, before any secret
  // reaches a child, so the signing phase makes no download. A template that
  // bakes both skips these installs.
  const onPath = (exe) => spawnSync("where", [exe], { encoding: "utf8", env: phaseEnvironment }).status === 0;
  if (!onPath("dotnet")) {
    console.log(`installing .NET SDK ${DOTNET_SDK.version} (build-time; a baked template skips this)...`);
    const dotnetDir = join(tmpdir(), "dotnet");
    const archive = join(tmpdir(), `dotnet-sdk-${DOTNET_SDK.version}-win-x64.zip`);
    const r = spawnSync("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command",
      `$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue'; `
      + `Invoke-WebRequest -UseBasicParsing -Uri '${DOTNET_SDK.url}' -OutFile '${archive}'; `
      + `if ((Get-FileHash '${archive}' -Algorithm SHA512).Hash.ToLowerInvariant() -ne '${DOTNET_SDK.sha512}') { throw '.NET SDK checksum does not match' }; `
      + `if (Test-Path '${dotnetDir}') { Remove-Item -Recurse -Force '${dotnetDir}' }; `
      + `Add-Type -AssemblyName System.IO.Compression.FileSystem; `
      + `[System.IO.Compression.ZipFile]::ExtractToDirectory('${archive}', '${dotnetDir}'); Remove-Item -Force '${archive}'`],
      { stdio: "inherit", env: phaseEnvironment });
    if (r.status !== 0) process.exit(r.status ?? 1);
    phaseEnvironment.DOTNET_ROOT = dotnetDir;
    phaseEnvironment.PATH = `${dotnetDir};${phaseEnvironment.PATH}`;
  }
  if (!onPath("sign")) {
    console.log("installing dotnet `sign` tool (build-time)...");
    const signDir = join(tmpdir(), "signtools");
    const r = spawnSync("dotnet", signToolInstallArgs(signDir), { stdio: "inherit", env: phaseEnvironment });
    if (r.status !== 0) process.exit(r.status ?? 1);
    phaseEnvironment.PATH = `${signDir};${phaseEnvironment.PATH}`;
  }

  // Tauri signs the app .exe (before packaging) and the NSIS installer with this
  // custom command; %1 is each file path.
  signArgs = ["--config", JSON.stringify({ bundle: { windows: { signCommand: tauriSignCommand(signingConfig) } } })];
  console.log("windows signing ENABLED (Azure Artifact Signing via dotnet `sign`)");
} else {
  console.log("windows signing SKIPPED: Azure credentials absent (unsigned build)");
}

const signFile = (file) => {
  if (!signing) return;
  const result = spawnSync("sign", signArguments(signingConfig, file), { stdio: "inherit", env: phaseEnvironment });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
};

// The record server, muniment-cli, reaches the runtime over unix sockets and
// ships on macOS and Linux alone until the Windows attach transport carries the record.
const runtime = join("src-tauri", "target", "release", "muniment-runtime.exe");
const runtimeBuild = spawnSync("cargo", [
  "build", "--manifest-path", "src-tauri/Cargo.toml", "--package", "muniment-runtime",
  "--release", "--locked",
], { stdio: "inherit", env: phaseEnvironment });
if (runtimeBuild.error) throw runtimeBuild.error;
if (runtimeBuild.status !== 0) process.exit(runtimeBuild.status ?? 1);
// The reader sidecar is Go with CGO off, a resource beside the runtime.
const reader = join("src-tauri", "target", "release", "muniment-reader.exe");
const readerBuild = spawnSync("powershell.exe", [
  "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(".github", "build-reader.ps1"), reader,
], { stdio: "inherit", env: { ...phaseEnvironment, GOOS: "windows", GOARCH: "amd64" } });
if (readerBuild.error) throw readerBuild.error;
if (readerBuild.status !== 0) process.exit(readerBuild.status ?? 1);

// Tauri signs bundled DLL resources in place. Keep the pinned, hash-checked
// inputs so later bundling passes validate and package the same upstream bits.
const runtimeDirectory = join("src-tauri", "third-party", "sherpa-onnx-v1.13.2", "windows-x86_64");
const runtimeFiles = ["onnxruntime.dll", "onnxruntime_providers_shared.dll", "sherpa-onnx-c-api.dll"];
const pristineRuntime = join(tmpdir(), `muniment-pristine-asr-runtime-${process.pid}`);
mkdirSync(pristineRuntime);
for (const file of runtimeFiles) copyFileSync(join(runtimeDirectory, file), join(pristineRuntime, file));
const restoreRuntime = () => {
  for (const file of runtimeFiles) copyFileSync(join(pristineRuntime, file), join(runtimeDirectory, file));
};

// The Chromium bootstrap loads the application DLL with its sandbox broker.
// Build once, stage that layout, then bundle without replacing the bootstrap.
run("build", "desktop application", "--verbose", "--no-bundle");
const libraryBuild = spawnSync("cargo", [
  "build", "--manifest-path", "src-tauri/Cargo.toml", "--package", "muniment-desktop",
  "--release", "--locked", "--lib", "--features", "tauri/custom-protocol",
], { stdio: "inherit", env: phaseEnvironment });
if (libraryBuild.error) throw libraryBuild.error;
if (libraryBuild.status !== 0) process.exit(libraryBuild.status ?? 1);
const cefConfig = join(tmpdir(), `muniment-cef-bundle-${process.pid}.json`);
const cefPackaging = spawnSync(process.execPath, [
  "scripts/package-cef-windows.mjs", "src-tauri/target/release/cef-app", "--installer-config", cefConfig,
], { stdio: "inherit", env: phaseEnvironment });
if (cefPackaging.error) throw cefPackaging.error;
if (cefPackaging.status !== 0) process.exit(cefPackaging.status ?? 1);

// Keep a distinct build for the Windows VM's same-version major-upgrade verification.
// The WiX template enables AllowSameVersionUpgrades; the current build remains the
// release artifact and is the only machine MSI uploaded.
const upgradeBaseMsi = join(dirname(msiDirectory), "machine-upgrade-base.msi");
// The upgrade-base is a throwaway fixture for the in-place-upgrade test —
// unsigned, so it runs before the signing phase and fetches the WiX toolset
// there when the template cache lacks it.
run("bundle", "machine upgrade-base MSI", "--verbose", "--bundles", "msi", "--config", "src-tauri/tauri.machine.conf.json", "--config", cefConfig);
await rename(await soleMsi(), upgradeBaseMsi);
// Tauri fetches NSIS on its first NSIS bundle. When the tool cache lacks it, an
// unsigned NSIS pass fetches it here; the signed pass below overwrites the file.
const nsisTool = join(process.env.LOCALAPPDATA ?? tmpdir(), "tauri", "NSIS", "makensis.exe");
if (signing && !existsSync(nsisTool)) {
  run("bundle", "NSIS toolset", "--verbose", "--bundles", "nsis", "--config", cefConfig);
  restoreRuntime();
}

// Signing phase: only the bundler, restores and the `sign` tool run from here,
// with the Azure credentials in their environment.
if (signing) {
  phaseEnvironment = { ...phaseEnvironment, ...signingEnvironment };
  // Do ONE real test-sign on a throwaway copy with output inherited, so the
  // real `sign` error surfaces (tauri's signCommand only reports "failed to
  // run …") before the bundling passes.
  const probeTarget = join(tmpdir(), "signprobe.exe");
  copyFileSync(process.execPath, probeTarget);
  console.log("preflight: test-signing a throwaway copy to surface the real error...");
  const probe = spawnSync("sign", signArguments(signingConfig, probeTarget), { stdio: "inherit", env: phaseEnvironment });
  if (probe.status !== 0) {
    console.error(`signing preflight FAILED (sign rc=${probe.status}) — see output above`);
    process.exit(probe.status ?? 1);
  }
  console.log("signing preflight OK");
}
signFile(runtime);
signFile(reader);

// Preserve the normal MSI while the machine bundling pass writes the fleet variant.
// signArgs signs the bootstrap, bundled DLL resources and NSIS installer.
run("bundle", "per-user installer", "--verbose", "--config", cefConfig, ...signArgs);
restoreRuntime();
const userMsi = await soleMsi();
const savedUserMsi = join(dirname(userMsi), `.${basename(userMsi)}.per-user`);
await rename(userMsi, savedUserMsi);
run("bundle", "machine MSI", "--verbose", "--bundles", "msi", "--config", "src-tauri/tauri.machine.conf.json", "--config", cefConfig, ...signArgs);
restoreRuntime();
rmSync(pristineRuntime, { recursive: true });
rmSync(cefConfig);
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
