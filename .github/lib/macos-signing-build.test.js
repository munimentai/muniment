import { afterEach, beforeEach, expect, it, vi } from "vitest";

const application = '  1) A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4 "Developer ID Application: Muniment (Y5DUNHQA74)"';
const installer = '  2) B1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4 "Developer ID Installer: Muniment (Y5DUNHQA74)"';
let spawn;
let identityResult;
let searchListResult;
let exitListeners;

beforeEach(() => {
  vi.resetModules();
  exitListeners = process.listeners("exit");
  for (const name of [
    "APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_TEAM_ID",
    "APPLE_API_KEY", "APPLE_API_KEY_ID", "APPLE_API_ISSUER",
  ]) vi.stubEnv(name, "test-value");
  vi.stubEnv("MACOS_SIGNING_ENABLED", "true");
  vi.spyOn(console, "log").mockImplementation(() => {});
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.spyOn(process.stdout, "write").mockReturnValue(true);
  vi.spyOn(process.stderr, "write").mockReturnValue(true);
  vi.spyOn(process, "exit").mockImplementation((status) => { throw new Error(`exit ${status}`); });
  vi.doMock("node:fs", () => {
    const fs = {
      existsSync: () => false,
      mkdtempSync: () => "/tmp/signing work",
      readFileSync: vi.fn(),
      rmSync: vi.fn(),
      writeFileSync: vi.fn(),
    };
    return { ...fs, default: fs };
  });
  vi.doMock("node:fs/promises", () => {
    const fs = { readdir: async () => [{ name: "asr.dylib", isDirectory: () => false, isFile: () => true }] };
    return { ...fs, default: fs };
  });
  identityResult = { status: 0, stdout: `${application}\n     1 valid identities found\n` };
  searchListResult = { status: 0, stdout: '    "/Users/builder/Library/Keychains/login.keychain-db"\n' };
  spawn = vi.fn((command, args) => {
    if (command === "security" && args[0] === "find-identity") {
      return args.includes("codesigning") ? identityResult : { status: 0, stdout: installer };
    }
    if (command === "security" && args[0] === "list-keychains" && !args.includes("-s")) return searchListResult;
    return { status: 0, stdout: "" };
  });
  vi.doMock("node:child_process", () => ({ spawnSync: spawn, default: { spawnSync: spawn } }));
});

afterEach(() => {
  for (const listener of process.listeners("exit")) {
    if (!exitListeners.includes(listener)) process.removeListener("exit", listener);
  }
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
  for (const module of ["node:fs", "node:fs/promises", "node:child_process"]) vi.doUnmock(module);
});

it("checks trust before signing the dylib and app, then submits for notarization", async () => {
  await import("../build-macos-app.mjs");
  const calls = spawn.mock.calls;
  const tauri = calls.find(([, args]) => args[0].endsWith("tauri.js"));
  expect(tauri[1]).toContain("--no-sign");
  const registration = calls.findIndex(([command, args]) => command === "security" && args.includes("-s") && args[0] === "list-keychains");
  expect(calls[registration][1]).toContain("/System/Library/Keychains/SystemRootCertificates.keychain");
  const preflight = calls.findIndex(([command, args]) => command === "security" && args.includes("codesigning"));
  const signing = calls.flatMap(([command, args], index) => command === "codesign" && args[0] === "--force" ? [index] : []);
  expect(registration).toBeLessThan(preflight);
  expect(signing).toHaveLength(3);
  expect(preflight).toBeLessThan(signing[0]);
  expect(calls[signing[0]][1].at(-1)).toMatch(/asr\.dylib$/);
  expect(calls[signing[2]][1].at(-1)).toMatch(/muniment\.app$/);
  const submission = calls.findIndex(([command, args]) => command === "xcrun" && args[0] === "notarytool" && args[1] === "submit");
  expect(submission).toBeGreaterThan(signing[2]);
});

it("stops before codesign and prints the failed identity check output", async () => {
  identityResult = { status: 0, stdout: "0 valid identities found", stderr: "Trust evaluation failed" };
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("0 valid identities found\nTrust evaluation failed");
  expect(spawn.mock.calls.some(([command]) => command === "codesign" || command === "xcrun")).toBe(false);
});

it("stops before replacing the search list when the list command fails", async () => {
  searchListResult = { status: 1, stdout: "", stderr: "Cannot read keychains" };
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(process.stderr.write).toHaveBeenCalledWith("Cannot read keychains");
  expect(spawn.mock.calls.some(([command, args]) => command === "security" && args[0] === "list-keychains" && args.includes("-s"))).toBe(false);
  expect(spawn.mock.calls.some(([command]) => command === "codesign")).toBe(false);
});
