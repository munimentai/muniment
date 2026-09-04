import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const PI_PACKAGES = [
  "npm:pi-web-access",
  "npm:pi-subagents",
  "npm:pi-background-tasks",
  "npm:pi-mcp-adapter",
];
export const PI_DEFAULT_TOOLS = ["read", "write", "edit", "bash", "grep", "find", "ls"];

export function renderPiSettings(destination) {
  writeFileSync(destination, `${JSON.stringify({ packages: PI_PACKAGES, defaultTools: PI_DEFAULT_TOOLS }, null, 2)}\n`);
}

export function buildPiAgent(root = process.cwd()) {
  const source = join(root, "src-tauri", "pi-agent");
  const destination = source;
  const npmDirectory = join(destination, "npm");
  rmSync(npmDirectory, { recursive: true, force: true });
  mkdirSync(npmDirectory, { recursive: true });
  cpSync(join(source, "package.json"), join(npmDirectory, "package.json"));
  cpSync(join(source, "package-lock.json"), join(npmDirectory, "package-lock.json"));
  mkdirSync(join(destination, ".pi"), { recursive: true });
  cpSync(join(root, ".pi", "mcp.json"), join(destination, ".pi", "mcp.json"));
  renderPiSettings(join(destination, "settings.json"));
  writeFileSync(join(destination, "bundle-version"), "0.84.4\n");

  const npm = process.platform === "win32" ? "npm.cmd" : "npm";
  const install = spawnSync(npm, ["ci", "--omit=dev", "--no-audit", "--no-fund"], {
    cwd: npmDirectory,
    stdio: "inherit",
  });
  if (install.error) throw install.error;
  if (install.status !== 0) process.exit(install.status ?? 1);

  for (const sourceName of PI_PACKAGES) {
    const packageName = sourceName.slice("npm:".length);
    const packageDirectory = join(npmDirectory, "node_modules", packageName);
    if (!readFileSync(join(packageDirectory, "package.json"), "utf8")) {
      throw new Error(`Pi package is unavailable: ${packageName}`);
    }
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) buildPiAgent();
