import assert from "node:assert/strict";
import test from "node:test";
import { WorkspaceOnboarding, type WorkspaceFolderLike } from "../src/workspace-onboarding";

const folder = (key: string, scheme = "file"): WorkspaceFolderLike =>
  ({ key, scheme, fsPath: `/repos/${key}` });

test("initializes defaults, overrides, and every local root", async () => {
  const calls: string[][] = [];
  const onboarding = new WorkspaceOnboarding(
    (item) => item.key === "two" ? "/memory/two" : "",
    async () => ({ onboardWorkspace: async (...args: string[]) => { calls.push(args); }, dispose() {} }),
    assert.fail,
  );
  await onboarding.initialize([folder("one"), folder("two"), folder("remote", "vscode-remote")]);
  assert.deepEqual(calls.sort(), [["/repos/one", "/repos/one"], ["/repos/two", "/memory/two"]]);
});

test("folder additions and changed resource overrides can be applied", async () => {
  const calls: string[][] = [];
  let override = "";
  const onboarding = new WorkspaceOnboarding(
    () => override,
    async () => ({ onboardWorkspace: async (...args: string[]) => { calls.push(args); }, dispose() {} }),
    assert.fail,
  );
  await onboarding.initializeFolder(folder("added"));
  override = "/new-memory";
  await onboarding.initializeFolder(folder("added"));
  assert.deepEqual(calls, [["/repos/added", "/repos/added"], ["/repos/added", "/new-memory"]]);
});

test("runtime and invalid override failures are bounded and reported", async () => {
  const messages: string[] = [];
  const invalid = new WorkspaceOnboarding(() => "relative", async () => { throw new Error(); },
    (message) => messages.push(message));
  await invalid.initialize([folder("one")]);
  const failed = new WorkspaceOnboarding(() => "", async () => { throw new Error("offline"); },
    (message) => messages.push(message));
  await failed.initialize([folder("two")]);
  assert.equal(messages.length, 2);
});

test("concurrent activation requests for one folder share initialization", async () => {
  let calls = 0;
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const onboarding = new WorkspaceOnboarding(() => "", async () => ({
    onboardWorkspace: async () => { calls++; await gate; }, dispose() {},
  }), assert.fail);
  const first = onboarding.initializeFolder(folder("one"));
  const second = onboarding.initializeFolder(folder("one"));
  release();
  await Promise.all([first, second]);
  assert.equal(calls, 1);
});

test("an override changed during initialization is applied after the stale request", async () => {
  const calls: string[][] = [];
  let override = "";
  let release!: () => void;
  const gate = new Promise<void>((resolve) => { release = resolve; });
  const onboarding = new WorkspaceOnboarding(() => override, async () => ({
    onboardWorkspace: async (...args: string[]) => {
      calls.push(args);
      if (calls.length === 1) await gate;
    },
    dispose() {},
  }), assert.fail);
  const first = onboarding.initializeFolder(folder("one"));
  override = "/changed";
  const changed = onboarding.initializeFolder(folder("one"));
  release();
  await Promise.all([first, changed]);
  assert.deepEqual(calls, [["/repos/one", "/repos/one"], ["/repos/one", "/changed"]]);
});
