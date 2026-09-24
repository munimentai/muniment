import { afterEach, beforeEach, expect, it, vi } from "vitest";

const application = '  1) A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4 "Developer ID Application: Muniment (Y5DUNHQA74)"';
const installer = '  2) B1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4 "Developer ID Installer: Muniment (Y5DUNHQA74)"';
let spawn;
let identityResult;
let searchListResult;
let exitListeners;
let clock;
let notaryResult;
let originalPath;
const submissionId = "103b0143-92d9-42a1-82e1-b05b8b7da7db";
const installerId = "203b0143-92d9-42a1-82e1-b05b8b7da7db";
const jsonResult = (value) => ({ status: 0, stdout: JSON.stringify(value) });

beforeEach(() => {
  vi.resetModules();
  // The signing phase narrows PATH to the system directories.
  originalPath = process.env.PATH;
  clock = 0;
  vi.spyOn(performance, "now").mockImplementation(() => clock);
  vi.doMock("node:timers/promises", () => {
    const timers = { setTimeout: vi.fn(async (delay) => { clock += delay; }) };
    return { ...timers, default: timers };
  });
  notaryResult = (args) => jsonResult({
    id: args[1] === "submit" ? (args[2].endsWith(".pkg") ? installerId : submissionId) : args[2],
    ...(args[1] === "info" ? { status: "Accepted" } : {}),
  });
  exitListeners = process.listeners("exit");
  // The credentials arrive on stdin through the signing-env reader, never in
  // the process environment.
  const credentials = Object.fromEntries([
    "APPLE_CERTIFICATE", "APPLE_CERTIFICATE_PASSWORD", "APPLE_TEAM_ID",
    "APPLE_API_KEY", "APPLE_API_KEY_ID", "APPLE_API_ISSUER",
  ].map((name) => [name, "test-value"]));
  vi.doMock("./signing-env.mjs", () => ({ readSigningEnvironment: vi.fn(() => credentials) }));
  vi.stubEnv("MACOS_SIGNING_ENABLED", "true");
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", "3600");
  vi.spyOn(console, "log").mockImplementation(() => {});
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.spyOn(process.stdout, "write").mockReturnValue(true);
  vi.spyOn(process.stderr, "write").mockReturnValue(true);
  vi.spyOn(process, "exit").mockImplementation((status) => { throw new Error(`exit ${status}`); });
  vi.doMock("node:fs", () => {
    const fs = {
      existsSync: () => false,
      mkdtempSync: () => "/tmp/signing work",
      readFileSync: vi.fn(() => Buffer.from("intermediate")),
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
  spawn = vi.fn((command, args, options) => {
    if (command === "xcrun" && args[0] === "notarytool") return notaryResult(args, options);
    if (command === "security" && args[0] === "find-identity") {
      return args.includes("codesigning") ? identityResult : { status: 0, stdout: installer };
    }
    if (command === "security" && args[0] === "list-keychains" && !args.includes("-s")) return searchListResult;
    return { status: 0, stdout: "" };
  });
  vi.doMock("node:child_process", () => ({ spawnSync: spawn, default: { spawnSync: spawn } }));
});

afterEach(() => {
  process.env.PATH = originalPath;
  for (const listener of process.listeners("exit")) {
    if (!exitListeners.includes(listener)) process.removeListener("exit", listener);
  }
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
  for (const module of ["node:fs", "node:fs/promises", "node:child_process", "node:timers/promises", "./signing-env.mjs"]) vi.doUnmock(module);
});

it.each([0, 700])("keeps the accepted path after %s seconds of setup", async (setupSeconds) => {
  clock = setupSeconds * 1000;
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", String(3600 - setupSeconds));
  await import("../build-macos-app.mjs");
  const calls = spawn.mock.calls;
  const tauri = calls.find(([, args]) => args[0].endsWith("tauri.js"));
  expect(tauri[1]).toContain("--no-sign");
  expect(tauri[1]).toContain("--no-bundle");
  const helper = calls.findIndex(([, args]) => args[0].endsWith("build-macos-cef-helper.mjs"));
  const bundle = calls.findIndex(([, args]) => args[0].endsWith("tauri.js") && args[1] === "bundle");
  expect(helper).toBeGreaterThan(calls.indexOf(tauri));
  expect(bundle).toBeGreaterThan(helper);
  expect(calls[bundle][1]).toContain("--no-sign");
  const registration = calls.findIndex(([command, args]) => command === "security" && args.includes("-s") && args[0] === "list-keychains");
  expect(calls[registration][1]).toContain("/System/Library/Keychains/SystemRootCertificates.keychain");
  const preflight = calls.findIndex(([command, args]) => command === "security" && args.includes("codesigning"));
  const signing = calls.flatMap(([command, args], index) => command === "codesign" && args[0] === "--force" ? [index] : []);
  expect(registration).toBeLessThan(preflight);
  // Package CEF before signing, and seal each nested binary before the app.
  const cefPackaging = calls.findIndex(([, args]) => args[0] === "scripts/package-cef-macos.mjs");
  expect(cefPackaging).toBeGreaterThan(bundle);
  expect(calls[cefPackaging][1]).toContain("--universal");
  expect(cefPackaging).toBeLessThan(signing[0]);
  expect(signing).toHaveLength(12);
  expect(preflight).toBeLessThan(signing[0]);
  const signedPaths = signing.map(index => {
    const args = calls[index][1];
    return args[args.indexOf("--sign") + 2];
  });
  expect(signedPaths[0]).toMatch(/asr\.dylib$/);
  expect(signedPaths[1]).toMatch(/Chromium Embedded Framework\.framework$/);
  for (const [offset, suffix] of ["", " (GPU)", " (Renderer)", " (Plugin)", " (Alerts)"].entries()) {
    expect(signedPaths[offset + 2]).toContain(`muniment CEF Helper${suffix}.app`);
  }
  expect(signedPaths[7]).toMatch(/LaunchServices\/muniment-runtime$/);
  expect(signedPaths[8]).toMatch(/LaunchServices\/muniment-cli$/);
  expect(signedPaths[9]).toMatch(/LaunchServices\/muniment-reader$/);
  expect(signedPaths[10]).toMatch(/MacOS\/muniment-cef-helper$/);
  expect(signedPaths[11]).toMatch(/muniment\.app$/);
  for (const index of [2, 3, 4, 5, 6, 10, 11]) {
    const args = calls[signing[index]][1];
    expect(args[args.indexOf("--entitlements") + 1]).toBe("src-tauri/packaging/entitlements.plist");
  }
  const submission = calls.findIndex(([command, args]) => command === "xcrun" && args[0] === "notarytool" && args[1] === "submit");
  expect(submission).toBeGreaterThan(signing.at(-1));
  const packaging = calls.slice(submission).filter(([command]) => ["xcrun", "ditto", "productbuild"].includes(command));
  expect(packaging.map(([command, args]) => command === "xcrun" ? args.slice(0, 2).join(" ") : command)).toEqual([
    "notarytool submit", "notarytool info", "stapler staple", "stapler validate",
    "ditto", "productbuild", "notarytool submit", "notarytool info", "stapler staple", "stapler validate",
  ]);
  expect(packaging[2][1].at(-1)).toMatch(/muniment\.app$/);
  expect(packaging[5][1]).toContain("--sign");
  expect(packaging[8][1].at(-1)).toMatch(/muniment\.pkg$/);
  expect(console.error).not.toHaveBeenCalled();
  expect(process.exit).not.toHaveBeenCalled();
});

it("keeps the Apple credentials out of the build environment and runs signing tools from system directories", async () => {
  const path = process.env.PATH;
  const paths = [];
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    paths.push([command, process.env.PATH]);
    return defaultSpawn(command, args, options);
  });
  await import("../build-macos-app.mjs");
  expect(process.env.APPLE_CERTIFICATE).toBeUndefined();
  for (const [, , options] of spawn.mock.calls) expect(options?.env?.APPLE_CERTIFICATE).toBeUndefined();
  const firstSigningTool = paths.findIndex(([command]) => command === "security");
  expect(paths.slice(0, firstSigningTool).every(([, value]) => value === path)).toBe(true);
  expect(paths.slice(firstSigningTool).every(([, value]) => value === "/usr/bin:/bin:/usr/sbin:/sbin")).toBe(true);
});

it("authorizes every signing tool before it uses the imported keys", async () => {
  await import("../build-macos-app.mjs");
  const calls = spawn.mock.calls;
  const imported = calls.findIndex(([command, args]) => command === "security" && args[0] === "import");
  const partitioned = calls.findIndex(([command, args]) => command === "security" && args[0] === "set-key-partition-list");
  const importArgs = calls[imported][1];
  const keychain = importArgs[importArgs.indexOf("-k") + 1];
  const trustedTools = importArgs.flatMap((arg, index) => arg === "-T" ? [importArgs[index + 1]] : []);
  const signers = calls.filter(([, args]) => args.includes("--sign"));
  expect([...new Set(signers.map(([command]) => `/usr/bin/${command}`))]).toEqual(trustedTools);
  expect(trustedTools).toEqual(["/usr/bin/codesign", "/usr/bin/productbuild"]);
  expect(importArgs).not.toContain("-A");
  expect(calls[partitioned][1]).toEqual([
    "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:,productbuild:", "-s", "-k", "muniment-ci-signing", keychain,
  ]);
  expect(imported).toBeLessThan(partitioned);
  for (const call of signers) expect(partitioned).toBeLessThan(calls.indexOf(call));
  const installerArgs = signers.find(([command]) => command === "productbuild")[1];
  expect(installerArgs[installerArgs.indexOf("--sign") + 1]).toBe("B1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4");
  expect(installerArgs[installerArgs.indexOf("--keychain") + 1]).toBe(keychain);
});

it.each(["import", "set-key-partition-list"])("stops before signing when %s fails", async (step) => {
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    if (command === "security" && args[0] === step) return { status: 1 };
    return defaultSpawn(command, args, options);
  });
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(spawn.mock.calls.some(([command]) => ["codesign", "productbuild", "xcrun"].includes(command))).toBe(false);
});

it("places the Developer ID G2 intermediate in the login keychain before signing and removes it on exit", async () => {
  await import("../build-macos-app.mjs");
  const calls = spawn.mock.calls;
  const login = "/Library/Keychains/login.keychain-db";
  const lookup = calls.findIndex(([command, args]) => command === "security" && args[0] === "find-certificate");
  const placement = calls.findIndex(([command, args]) => command === "security" && args[0] === "import" && args.at(-1).endsWith(login));
  const signing = calls.findIndex(([command]) => command === "codesign");
  expect(calls[lookup][1]).toEqual(["find-certificate", "-Z", "-c", "Developer ID Certification Authority", expect.stringMatching(/\/Library\/Keychains\/login\.keychain-db$/)]);
  expect(calls[placement][1]).toEqual(["import", ".github/certs/DeveloperIDG2CA.cer", "-k", calls[lookup][1].at(-1)]);
  expect(lookup).toBeLessThan(placement);
  expect(placement).toBeLessThan(signing);
  expect(calls.some(([command, args]) => command === "security" && args[0] === "import" && args.at(-1).endsWith(".cer") && !args.at(-1).endsWith(login))).toBe(false);
  for (const listener of process.listeners("exit")) if (!exitListeners.includes(listener)) listener();
  const removal = calls.find(([command, args]) => command === "security" && args[0] === "delete-certificate");
  expect(removal[1]).toEqual(["delete-certificate", "-Z", "CE03A80127FF0AFF9657B1B049FED435174E88B3", calls[lookup][1].at(-1)]);
});

it("keeps an intermediate the login keychain already holds", async () => {
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    if (command === "security" && args[0] === "find-certificate") return { status: 0, stdout: "SHA-1 hash: CE03A80127FF0AFF9657B1B049FED435174E88B3\n" };
    return defaultSpawn(command, args, options);
  });
  await import("../build-macos-app.mjs");
  for (const listener of process.listeners("exit")) if (!exitListeners.includes(listener)) listener();
  const calls = spawn.mock.calls;
  expect(calls.some(([command, args]) => command === "security" && args[0] === "import" && args[1].endsWith(".cer"))).toBe(false);
  expect(calls.some(([command, args]) => command === "security" && args[0] === "delete-certificate")).toBe(false);
  expect(calls.some(([command]) => command === "codesign")).toBe(true);
});

it("keeps unsigned packaging free of keychain access", async () => {
  vi.stubEnv("MACOS_SIGNING_ENABLED", "false");
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 0");
  expect(spawn.mock.calls.some(([command]) => ["security", "codesign", "xcrun"].includes(command))).toBe(false);
  const installerCall = spawn.mock.calls.find(([command]) => command === "productbuild");
  expect(installerCall[1]).not.toContain("--sign");
  expect(installerCall[1].at(-1)).toMatch(/muniment\.pkg$/);
});

it.each([
  [false, 0], [true, 0], [false, 700], [true, 700],
])("names a queue timeout for installer=%s after %s seconds of setup", async (installer, setupSeconds) => {
  clock = setupSeconds * 1000;
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", String(3600 - setupSeconds));
  const waitSeconds = 2400 - setupSeconds;
  notaryResult = (args) => {
    const id = args[1] === "submit" ? (args[2].endsWith(".pkg") ? installerId : submissionId) : args[2];
    return jsonResult({ id, status: installer && id === submissionId ? "Accepted" : "In Progress" });
  };
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(clock).toBe((setupSeconds + waitSeconds) * 1000);
  expect(clock).toBeLessThanOrEqual(3000_000);
  expect(process.exit).toHaveBeenCalledWith(1);
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    `macOS notarization FAILED cause=notarization-timeout submission_id=${installer ? installerId : submissionId} last_status="In Progress" waited_seconds=${waitSeconds}`,
  ));
  expect(console.error.mock.invocationCallOrder[0]).toBeLessThan(process.exit.mock.invocationCallOrder[0]);
  expect(spawn.mock.calls.filter(([, args]) => args[0] === "stapler" && args[1] === "staple")).toHaveLength(installer ? 1 : 0);
  for (const [, args, options] of spawn.mock.calls.filter(([, args]) => args[0] === "notarytool")) {
    expect(args).not.toContain("--wait");
    expect(options.timeout).toBeGreaterThan(0);
    expect(options.timeout).toBeLessThanOrEqual(300_000);
    expect(options.killSignal).toBe("SIGKILL");
  }
});

it.each([0, 700])("shares the installer deadline with compilation and %s seconds of setup", async (setupSeconds) => {
  clock = setupSeconds * 1000;
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", String(3600 - setupSeconds));
  const deadlineSeconds = 2400;
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    if (args[0].endsWith("tauri.js")) clock += 600_000;
    return defaultSpawn(command, args, options);
  });
  notaryResult = (args) => {
    const id = args[1] === "submit" ? (args[2].endsWith(".pkg") ? installerId : submissionId) : args[2];
    return jsonResult({ id, status: id === submissionId && clock >= (setupSeconds + 1200) * 1000 ? "Accepted" : "In Progress" });
  };
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(clock).toBe(deadlineSeconds * 1000);
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    `submission_id=${installerId} last_status="In Progress" waited_seconds=${deadlineSeconds - setupSeconds - 1200}`,
  ));
});

it("does not submit after compilation exhausts the deadline", async () => {
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    if (args[0].endsWith("tauri.js")) clock = 2400_000;
    return defaultSpawn(command, args, options);
  });
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    'cause=notarization-timeout submission_id=unknown last_status="unknown" waited_seconds=0',
  ));
  expect(spawn.mock.calls.some(([, args]) => args[0] === "notarytool")).toBe(false);
});

it.each(["", " ", "NaN", "Infinity", "3601", "1.5", "1e3", "-9007199254740992"])("rejects an invalid remaining build budget %s", async (budget) => {
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", budget);
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("MACOS_BUILD_REMAINING_SECONDS must be an integer at most 3600");
  expect(spawn).not.toHaveBeenCalled();
});

it.each(["1200", "600", "0", "-1"])("does not submit with an exhausted remaining build budget %s", async (budget) => {
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", budget);
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    'cause=notarization-timeout submission_id=unknown last_status="unknown" waited_seconds=0',
  ));
  expect(spawn.mock.calls.some(([, args]) => args[0] === "notarytool")).toBe(false);
});

it.each([undefined, NaN, Infinity, "2400000"])("rejects an invalid deadline %s before a request", async (deadline) => {
  const { notarize } = await import("./macos-signing.mjs");
  await expect(notarize({}, "muniment.app.zip", "/tmp/key.p8", deadline)).rejects.toThrow("cause=invalid-deadline");
  expect(spawn).not.toHaveBeenCalled();
});

it.each(["Invalid", "Rejected"])("fails without stapling when Apple returns %s", async (status) => {
  notaryResult = () => jsonResult({ id: submissionId, status });
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    `cause=notarization-rejected submission_id=${submissionId} last_status="${status}" waited_seconds=0`,
  ));
  expect(spawn.mock.calls.some(([, args]) => args[0] === "stapler")).toBe(false);
});

it.each(["", "null", "{}", '{"id":"--help"}'])("rejects an invalid submission response %s", async (stdout) => {
  notaryResult = () => ({ status: 0, stdout });
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining("cause=invalid-response submission_id=unknown"));
  expect(spawn.mock.calls.some(([, args]) => args[0] === "notarytool" && args[1] === "info")).toBe(false);
});

it.each([
  jsonResult(null), jsonResult({ id: submissionId }),
  jsonResult({ id: installerId, status: "Accepted" }),
  jsonResult({ id: submissionId, status: "Unexpected\nAccepted" }),
  { status: 1, stdout: JSON.stringify({ id: submissionId, status: "Accepted" }) },
  { status: null, error: new Error("spawn failed") },
])("rejects invalid or failed status responses without stapling", async (result) => {
  notaryResult = (args) => args[1] === "submit" ? jsonResult({ id: submissionId }) : result;
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(`submission_id=${submissionId} last_status="unknown"`));
  expect(spawn.mock.calls.some(([, args]) => args[0] === "stapler")).toBe(false);
});

it("keeps the last status when a later request stalls at the deadline", async () => {
  notaryResult = (args, options) => {
    if (args[1] === "info" && clock >= 2355_000) {
      clock += options.timeout;
      return { status: null, error: Object.assign(new Error("timed out"), { code: "ETIMEDOUT" }) };
    }
    return jsonResult({ id: submissionId, status: "In Progress" });
  };
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(clock).toBe(2400_000);
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    `cause=notarization-timeout submission_id=${submissionId} last_status="In Progress" waited_seconds=2400`,
  ));
});

it.each([2399_999, 2400_000])("accepts only a status inside the deadline at %s milliseconds", async (acceptedAt) => {
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    if (args[0].endsWith("tauri.js")) clock = 2390_000;
    return defaultSpawn(command, args, options);
  });
  notaryResult = (args) => {
    if (args[1] === "info") clock = acceptedAt;
    const id = args[1] === "submit" ? (args[2].endsWith(".pkg") ? installerId : submissionId) : args[2];
    return jsonResult({ id, status: "Accepted" });
  };
  if (acceptedAt < 2400_000) {
    await import("../build-macos-app.mjs");
    expect(process.exit).not.toHaveBeenCalled();
  } else {
    await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
    expect(console.error).toHaveBeenCalledWith(expect.stringContaining("cause=notarization-timeout"));
  }
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
