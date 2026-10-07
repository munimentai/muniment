import { afterEach, beforeEach, expect, it, vi } from "vitest";

const application = '  1) A1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4 "Developer ID Application: Muniment (Y5DUNHQA74)"';
const installer = '  2) B1B2C3D4E5F60718293A4B5C6D7E8F90A1B2C3D4 "Developer ID Installer: Muniment (Y5DUNHQA74)"';
let spawn;
let exec;
let delay;
let packageDmg;
let identityResult;
let searchListResult;
let exitListeners;
let clock;
let notaryResult;
let originalPath;
let variantCount;
const submissionId = "103b0143-92d9-42a1-82e1-b05b8b7da7db";
const installerId = "203b0143-92d9-42a1-82e1-b05b8b7da7db";
const jsonResult = (value) => ({ status: 0, stdout: JSON.stringify(value) });

beforeEach(() => {
  vi.resetModules();
  variantCount = 1;
  packageDmg = vi.fn();
  vi.doMock("./macos-variants.mjs", () => ({
    prepareMacosVariants: base => ["", "-arm64", "-x64"].slice(0, variantCount).map(suffix => ({
      suffix, app: `${base}/macos/${suffix ? `${suffix.slice(1)}/` : ""}muniment.app`,
      zip: `${base}/macos/muniment${suffix}.app.zip`, pkg: `${base}/pkg/muniment${suffix}.pkg`, dmg: `${base}/dmg/muniment${suffix}.dmg`,
    })),
    packageMacosDmg: (...args) => packageDmg(...args),
  }));
  // The signing phase narrows PATH to the system directories.
  originalPath = process.env.PATH;
  clock = 0;
  vi.spyOn(performance, "now").mockImplementation(() => clock);
  const events = [];
  let scheduled = false;
  const tick = () => {
    scheduled = false;
    const next = Math.min(...events.map(event => event.at));
    clock = Math.max(clock, next);
    const ready = events.filter(event => event.at <= clock);
    for (const event of ready) events.splice(events.indexOf(event), 1);
    for (const event of ready) event.resolve();
    if (events.length) { scheduled = true; setImmediate(tick); }
  };
  delay = (milliseconds) => new Promise(resolve => {
    events.push({ at: clock + milliseconds, resolve });
    if (!scheduled) { scheduled = true; setImmediate(tick); }
  });
  vi.doMock("node:timers/promises", () => {
    const timers = { setTimeout: vi.fn(delay) };
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
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", "4800");
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
  exec = vi.fn((command, args, options, callback) => {
    Promise.resolve(spawn(command, args, options)).then(result => {
      const error = result.error ?? (result.status === 0 ? null : new Error("notarytool failed"));
      callback(error, result.stdout ?? "", result.stderr ?? "");
    });
  });
  const sync = (...args) => {
    if (args[0] === "xcrun" && args[1][0] === "notarytool") throw new Error("Notarization must use nonblocking requests");
    return spawn(...args);
  };
  vi.doMock("node:child_process", () => ({ spawnSync: sync, execFile: exec, default: { spawnSync: sync, execFile: exec } }));
});

afterEach(() => {
  process.env.PATH = originalPath;
  for (const listener of process.listeners("exit")) {
    if (!exitListeners.includes(listener)) process.removeListener("exit", listener);
  }
  vi.restoreAllMocks();
  vi.unstubAllEnvs();
  for (const module of ["node:fs", "node:fs/promises", "node:child_process", "node:timers/promises", "./signing-env.mjs", "./macos-variants.mjs"]) vi.doUnmock(module);
});

it.each([0, 700])("keeps the accepted path after %s seconds of setup", async (setupSeconds) => {
  clock = setupSeconds * 1000;
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", String(4800 - setupSeconds));
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
  expect(signing).toHaveLength(13);
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
  expect(submission).toBeGreaterThan(signing[11]);
  const packaging = calls.slice(submission).filter(([command]) => ["xcrun", "ditto", "productbuild"].includes(command));
  expect(packaging.map(([command, args]) => command === "xcrun" ? args.slice(0, 2).join(" ") : command)).toEqual([
    "notarytool submit", "notarytool info", "stapler staple", "stapler validate",
    "ditto", "productbuild", "notarytool submit", "notarytool submit", "notarytool info", "notarytool info",
    "stapler staple", "stapler validate", "stapler staple", "stapler validate",
  ]);
  expect(packaging[2][1].at(-1)).toMatch(/muniment\.app$/);
  expect(packaging[5][1]).toContain("--sign");
  expect(packaging[10][1].at(-1)).toMatch(/muniment\.pkg$/);
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

it("does not expose certificate arguments when import cannot start", async () => {
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    if (command === "security" && args[0] === "import") {
      return { status: null, error: Object.assign(new Error("secret test-value"), { code: "ENOENT", spawnargs: args }) };
    }
    return defaultSpawn(command, args, options);
  });
  await expect(import("../build-macos-app.mjs")).rejects.toThrow(/^security failed \(ENOENT\)$/);
  expect(console.error.mock.calls.flat().join("\n")).not.toContain("test-value");
  expect(spawn.mock.calls.some(([command]) => command === "codesign")).toBe(false);
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
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", String(4800 - setupSeconds));
  const waitSeconds = 4800 - setupSeconds - 600 - 300;
  notaryResult = (args) => {
    const id = args[1] === "submit" ? (args[2].endsWith(".pkg") || args[2].endsWith(".dmg") ? installerId : submissionId) : args[2];
    return jsonResult({ id, status: installer && id === submissionId ? "Accepted" : "In Progress" });
  };
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(clock).toBe((setupSeconds + waitSeconds) * 1000);
  expect(clock).toBe(3900_000);
  expect(process.exit).toHaveBeenCalledWith(1);
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    `macOS notarization FAILED cause=notarization-timeout submission_id=${installer ? installerId : submissionId} last_status="In Progress" waited_seconds=${waitSeconds}`,
  ));
  expect(console.error.mock.invocationCallOrder[0]).toBeLessThan(process.exit.mock.invocationCallOrder[0]);
  expect(spawn.mock.calls.filter(([, args]) => args[0] === "stapler" && args[1] === "staple")).toHaveLength(installer ? 1 : 0);
  for (const [, args, options] of spawn.mock.calls.filter(([, args]) => args[0] === "notarytool")) {
    expect(args).not.toContain("--wait");
    expect(options.timeout).toBeGreaterThan(0);
    expect(options.timeout).toBeLessThanOrEqual(args[1] === "submit" ? 600_000 : 60_000);
    expect(options.killSignal).toBe("SIGKILL");
  }
});

it.each([[0, false], [700, false], [700, true]])("completes nine timed submissions after %s seconds of setup with shared bandwidth=%s", async (setupSeconds, sharedBandwidth) => {
  variantCount = 3;
  clock = setupSeconds * 1000;
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", String(4800 - setupSeconds));
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    if (args[0].endsWith("tauri.js")) clock += 600_000;
    if (command === "codesign" && args[0] === "--force") clock += 5_000;
    if (command === "ditto") clock += 30_000;
    if (command === "productbuild") clock += 60_000;
    return defaultSpawn(command, args, options);
  });
  packageDmg.mockImplementation(() => { clock += 80_000; });
  const pending = new Map();
  const batches = [];
  let active = 0;
  notaryResult = async (args) => {
    if (args[1] === "submit") {
      active++;
      batches.push(active);
      const id = `${String(pending.size + 1).padStart(8, "0")}-92d9-42a1-82e1-b05b8b7da7db`;
      const uploadTime = 90_000 * (sharedBandwidth ? active : 1);
      pending.set(id, clock + uploadTime + 60_000);
      await delay(uploadTime);
      active--;
      return jsonResult({ id });
    }
    await delay(5_000);
    return jsonResult({ id: args[2], status: clock >= pending.get(args[2]) ? "Accepted" : "In Progress" });
  };
  await import("../build-macos-app.mjs");
  expect(batches).toEqual([1, 2, 3, 1, 2, 3, 4, 5, 6]);
  expect(pending.size).toBe(9);
  expect(exec.mock.calls.filter(([, args]) => args[1] === "submit")).toHaveLength(9);
  expect(clock).toBe((setupSeconds + (sharedBandwidth ? 2935 : 2305)) * 1000);
  // With setup, this workload exceeds the old 2400-second deadline even with parallel uploads.
  if (setupSeconds) expect(clock).toBeGreaterThan(2400_000);
  expect(clock).toBeLessThan(4200_000);
  expect(console.log.mock.calls.filter(([message]) => message.includes('last_status="Accepted"'))).toHaveLength(9);
  expect(spawn.mock.calls.filter(([, args]) => args[0] === "stapler" && args[1] === "validate")).toHaveLength(9);
  expect(spawn.mock.calls.filter(([cmd]) => cmd === "pkgutil")).toHaveLength(3);
  expect(process.exit).not.toHaveBeenCalled();
});

it.each([false, true])("replays the slow nightly queue with a stalled installer=%s", async (stalled) => {
  variantCount = 3;
  clock = 29_000;
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", "4771");
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    if (args[0].endsWith("tauri.js")) clock += 550_000;
    if (command === "codesign" && args[0] === "--force") clock += 1_000;
    if (command === "ditto") clock += args.includes("--sequesterRsrc") ? 30_000 : 50_000;
    if (command === "productbuild") clock += 40_000;
    if (command === "mkdir") clock += 2_000;
    if (args[0] === "stapler") clock += args[1] === "staple" ? 5_000 : 1_000;
    return defaultSpawn(command, args, options);
  });
  packageDmg.mockImplementation(() => { clock += 10_000; });
  const submissions = new Map();
  // Match the nightly's compile, app queue, and packaging times before the installer queue.
  const queueSeconds = [386, 462, 711, 1500, 1034, 1500, 1500, 1061, 1007];
  notaryResult = async (args) => {
    if (args[1] === "submit") {
      const index = submissions.size;
      const id = `${String(index + 1).padStart(8, "0")}-92d9-42a1-82e1-b05b8b7da7db`;
      submissions.set(id, { archive: args[2], started: clock, accepted: clock + queueSeconds[index] * 1000 });
      await delay(index < 3 ? 86_000 : 95_000);
      return jsonResult({ id });
    }
    await delay(5_000);
    const submission = submissions.get(args[2]);
    const stuck = stalled && submission.archive.endsWith("muniment-arm64.pkg");
    const accepted = !stuck && clock >= submission.accepted;
    if (accepted) submission.reportedAcceptedAt = clock;
    return jsonResult({ id: args[2], status: accepted ? "Accepted" : "In Progress" });
  };
  const build = import("../build-macos-app.mjs");
  if (stalled) await expect(build).rejects.toThrow("exit 1");
  else await build;
  const entries = [...submissions.entries()];
  expect(entries).toHaveLength(9);
  expect(entries.slice(0, 3).map(([, value]) => value.started)).toEqual(Array(3).fill(1315_000));
  expect(entries.slice(3).map(([, value]) => value.started)).toEqual(Array(6).fill(2289_000));
  const calls = spawn.mock.calls;
  expect(calls.filter(([cmd, args]) => cmd === "productbuild" && args.includes("--sign"))).toHaveLength(3);
  expect(calls.filter(([cmd, args]) => cmd === "codesign" && args.includes("--verify"))).toHaveLength(6);
  expect(calls.filter(([cmd]) => cmd === "pkgutil")).toHaveLength(3);
  const validated = calls.filter(([, args]) => args[0] === "stapler" && args[1] === "validate");
  if (stalled) {
    expect(clock).toBe(3900_000);
    const [id] = entries.find(([, value]) => value.archive.endsWith("muniment-arm64.pkg"));
    expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
      `cause=notarization-timeout submission_id=${id} last_status="In Progress" waited_seconds=1611`,
    ));
    expect(validated).toHaveLength(3);
    expect(console.log.mock.calls.some(([message]) => message.startsWith("kept "))).toBe(false);
  } else {
    const notaryDeadline = 3900_000;
    for (const [, submission] of entries) {
      expect(submission.reportedAcceptedAt).toBeLessThan(notaryDeadline);
    }
    const slowInstaller = entries[3][1];
    expect(slowInstaller.reportedAcceptedAt - slowInstaller.started).toBe(1500_000);
    expect(slowInstaller.reportedAcceptedAt).toBe(3789_000);
    expect(notaryDeadline - slowInstaller.reportedAcceptedAt).toBe(111_000);
    expect(clock).toBe(3825_000);
    expect(clock).toBeLessThan(4200_000);
    expect(validated).toHaveLength(9);
    expect(console.log.mock.calls.filter(([message]) => message.includes('last_status="Accepted"'))).toHaveLength(9);
    expect(console.log).toHaveBeenCalledWith(expect.stringContaining('last_status="Accepted" waited_seconds=1500'));
    const downloads = entries.slice(3).map(([, submission]) => submission.archive);
    for (const file of downloads) {
      const signing = calls.findIndex(([cmd, args]) => file.endsWith(".pkg")
        ? cmd === "productbuild" && args.includes("--sign") && args.at(-1) === file
        : cmd === "codesign" && args.includes("--sign") && args.at(-1) === file);
      const verification = calls.findIndex(([cmd, args]) => file.endsWith(".pkg")
        ? cmd === "pkgutil" && args.includes("--check-signature") && args.at(-1) === file
        : cmd === "codesign" && args.includes("--verify") && args.at(-1) === file);
      const staple = calls.findIndex(([, args]) => args[0] === "stapler" && args[1] === "staple" && args.at(-1) === file);
      const validation = calls.findIndex(([, args]) => args[0] === "stapler" && args[1] === "validate" && args.at(-1) === file);
      expect(signing).toBeGreaterThan(-1);
      expect(verification).toBeGreaterThan(signing);
      expect(staple).toBeGreaterThan(verification);
      expect(validation).toBeGreaterThan(staple);
    }
    expect(console.log.mock.calls.filter(([message]) => message.startsWith("kept "))).toEqual(
      downloads.map(file => [`kept ${file} (signed + notarized + stapled)`]),
    );
    expect(console.error).not.toHaveBeenCalled();
    expect(process.exit).not.toHaveBeenCalled();
  }
});

it("does not submit after compilation exhausts the deadline", async () => {
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    if (args[0].endsWith("tauri.js")) clock = 4200_000;
    return defaultSpawn(command, args, options);
  });
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("cause=build-timeout");
  expect(spawn.mock.calls.some(([, args]) => args[0] === "notarytool")).toBe(false);
});

it.each(["", " ", "NaN", "Infinity", "4801", "1.5", "1e3", "-9007199254740992"])("rejects an invalid remaining build budget %s", async (budget) => {
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", budget);
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("MACOS_BUILD_REMAINING_SECONDS must be an integer at most 4800");
  expect(spawn).not.toHaveBeenCalled();
});

it.each(["600", "0", "-1"])("does not build with an exhausted remaining build budget %s", async (budget) => {
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", budget);
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("cause=build-timeout");
  expect(spawn).not.toHaveBeenCalled();
});

it.each([601, 899, 900, 901, 1200])("preserves both reserves with %s seconds of build budget", async (seconds) => {
  vi.stubEnv("MACOS_BUILD_REMAINING_SECONDS", String(seconds));
  notaryResult = () => jsonResult({ id: submissionId, status: "In Progress" });
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  const waitSeconds = Math.max(0, seconds - 600 - 300);
  expect(clock).toBe(waitSeconds * 1000);
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    `cause=notarization-timeout submission_id=${waitSeconds ? submissionId : "unknown"} last_status=${waitSeconds ? '"In Progress"' : '"unknown"'} waited_seconds=${waitSeconds}`,
  ));
  if (!waitSeconds) expect(exec).not.toHaveBeenCalled();
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
    if (args[1] === "info" && clock >= 3855_000) {
      clock += options.timeout;
      return { status: null, error: Object.assign(new Error("timed out"), { code: "ETIMEDOUT" }) };
    }
    return jsonResult({ id: submissionId, status: "In Progress" });
  };
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(clock).toBe(3900_000);
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    `cause=notarization-timeout submission_id=${submissionId} last_status="In Progress" waited_seconds=3900`,
  ));
});

it.each([1199_999, 1200_000])("accepts only a status inside the deadline at %s milliseconds", async (acceptedAt) => {
  notaryResult = (args) => {
    if (args[1] === "info") clock = acceptedAt;
    return jsonResult({ id: submissionId, status: "Accepted" });
  };
  const { notarize } = await import("./macos-signing.mjs");
  const result = notarize({}, "muniment.app.zip", "/tmp/key.p8", 1200_000);
  if (acceptedAt < 1200_000) await expect(result).resolves.toBeUndefined();
  else await expect(result).rejects.toThrow(`cause=notarization-timeout submission_id=${submissionId} last_status="Accepted" waited_seconds=1200`);
});

it.each(["productbuild", "hdiutil"])("bounds %s packaging by the build deadline", async (command) => {
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((cmd, args, options) => {
    if (cmd === command) {
      expect(options.timeout).toBe(4200_000 - clock);
      expect(options.killSignal).toBe("SIGKILL");
      clock += options.timeout;
      return { status: null, error: Object.assign(new Error("timed out"), { code: "ETIMEDOUT" }) };
    }
    return defaultSpawn(cmd, args, options);
  });
  packageDmg.mockImplementation((app, output, run) => run("hdiutil", ["create", output]));
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("cause=build-timeout");
  expect(exec.mock.calls.filter(([, args]) => args[1] === "submit")).toHaveLength(1);
});

it.each(["--check-signature", "--verify", "staple", "validate"])("fails when installer verification %s fails", async (step) => {
  const defaultSpawn = spawn.getMockImplementation();
  spawn.mockImplementation((command, args, options) => {
    if (args.includes(step) && /\.(pkg|dmg)$/.test(args.at(-1))) return { status: 1 };
    return defaultSpawn(command, args, options);
  });
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(console.log.mock.calls.some(([message]) => message.startsWith("kept "))).toBe(false);
});

it("waits for the other submissions before a rejected Intel installer fails the batch", async () => {
  variantCount = 3;
  const archives = new Map();
  notaryResult = async (args) => {
    if (args[1] === "submit") {
      const id = `${String(archives.size + 1).padStart(8, "0")}-92d9-42a1-82e1-b05b8b7da7db`;
      archives.set(id, args[2]);
      await delay(30_000);
      return jsonResult({ id });
    }
    const rejected = archives.get(args[2]).endsWith("muniment-x64.pkg");
    await delay(rejected ? 5_000 : 20_000);
    return jsonResult({ id: args[2], status: rejected ? "Rejected" : "Accepted" });
  };
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(clock).toBe(100_000);
  expect(archives.size).toBe(9);
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    'submission_id=00000008-92d9-42a1-82e1-b05b8b7da7db last_status="Rejected" waited_seconds=35',
  ));
  expect(console.log.mock.calls.filter(([message]) => message.includes('last_status="Accepted"'))).toHaveLength(8);
  expect(spawn.mock.calls.filter(([, args]) => args[0] === "stapler" && args[1] === "validate")).toHaveLength(3);
});

it.each([false, true])("reports an upload timeout with its available submission ID=%s", async (hasId) => {
  notaryResult = async (args, options) => {
    expect(args[1]).toBe("submit");
    await delay(options.timeout);
    return {
      status: null, error: Object.assign(new Error("secret test-value"), { killed: true }),
      stdout: hasId ? JSON.stringify({ id: submissionId }) : "secret test-value", stderr: "secret test-value",
    };
  };
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(clock).toBe(600_000);
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining(
    `cause=notarization-timeout submission_id=${hasId ? submissionId : "unknown"} last_status="unknown" waited_seconds=600`,
  ));
  expect(console.error.mock.calls.flat().join("\n")).not.toContain("test-value");
  expect(process.stdout.write).not.toHaveBeenCalled();
  expect(process.stderr.write).not.toHaveBeenCalled();
  expect(spawn.mock.calls.some(([, args]) => args[0] === "stapler")).toBe(false);
});

it("does not print credentials when the request cannot start", async () => {
  exec.mockImplementation(() => { throw new Error("secret test-value"); });
  await expect(import("../build-macos-app.mjs")).rejects.toThrow("exit 1");
  expect(console.error).toHaveBeenCalledWith(expect.stringContaining("cause=notarytool-failed"));
  expect(console.error.mock.calls.flat().join("\n")).not.toContain("test-value");
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


it("signs all three app variants and their disk images before publication", async () => {
  variantCount = 3;
  await import("../build-macos-app.mjs");
  const calls = spawn.mock.calls;
  for (const suffix of ["", "-arm64", "-x64"]) {
    for (const format of [".pkg", ".dmg"]) {
      const file = `muniment${suffix}${format}`;
      expect(calls.some(([cmd, args]) => cmd === "xcrun" && args[0] === "stapler" && args[1] === "validate" && args.at(-1).endsWith(file))).toBe(true);
    }
  }
  expect(calls.filter(([cmd, args]) => cmd === "productbuild" && args.includes("--sign"))).toHaveLength(3);
  expect(calls.filter(([cmd, args]) => cmd === "codesign" && args.includes("--deep") && args.includes("--verify"))).toHaveLength(3);
});
