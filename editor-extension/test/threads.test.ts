import assert from "node:assert/strict";
import test from "node:test";
import { OPEN_THREAD_COMMAND, ThreadsModel, threadOpenCommand, type AttachConnector } from "../src/threads";
import {
  AttachTransportError,
  type AttachConnection,
  type JsonValue,
  type RunStartAccepted,
  type RunStreamSubscription,
  type ThreadListPage,
} from "../src/transport";

class FakeConnection implements AttachConnection {
  readonly capability = "not-shown";
  readonly expiresInSeconds = 3600;
  readonly idleTimeoutSeconds = 900;
  listCalls: Array<string | undefined> = [];
  disposed = false;
  startCalls: Array<{ text: string; context: JsonValue | undefined }> = [];
  streamCalls: Array<{ runId: string; afterRunSeq: number }> = [];
  streamResult: Promise<RunStreamSubscription> | undefined;
  startResult: Promise<RunStartAccepted> = Promise.resolve({
    runId: "run-1",
    committedSeq: 1,
    acceptedAt: "2026-07-18T12:00:00Z",
  });

  constructor(private readonly page: ThreadListPage) {}

  async listThreads(cursor?: string): Promise<ThreadListPage> {
    this.listCalls.push(cursor);
    return this.page;
  }

  async openThread(): Promise<never> {
    throw new Error("not used");
  }

  async startRun(text: string, context?: JsonValue): Promise<RunStartAccepted> {
    this.startCalls.push({ text, context });
    return this.startResult;
  }

  async streamRun(runId: string, afterRunSeq: number): Promise<RunStreamSubscription> {
    this.streamCalls.push({ runId, afterRunSeq });
    if (!this.streamResult) throw new Error("not configured");
    return this.streamResult;
  }

  dispose(): void {
    this.disposed = true;
  }
}

function connectorFor(...connections: FakeConnection[]): AttachConnector {
  let index = 0;
  return async () => connections[index++];
}

function stateKind(model: ThreadsModel): string {
  return model.state.kind;
}

test("projects the first thread page into native view data without paging", async () => {
  const connection = new FakeConnection({
    threads: [
      { threadId: "thread-1", title: "First thread", updatedAt: "2026-07-18T12:00:00Z" },
      { threadId: "thread-2", title: "Second thread", updatedAt: "2026-07-17T12:00:00Z" },
    ],
    nextCursor: "there-is-another-page",
  });
  const model = new ThreadsModel(connectorFor(connection), (value) => `Updated ${value}`);

  await model.refresh();

  assert.deepEqual(model.state, {
    kind: "ready",
    threads: [
      { threadId: "thread-1", title: "First thread", description: "Updated 2026-07-18T12:00:00Z" },
      { threadId: "thread-2", title: "Second thread", description: "Updated 2026-07-17T12:00:00Z" },
    ],
  });
  assert.deepEqual(connection.listCalls, [undefined]);
  model.dispose();
});

test("projects a ready row to an open command carrying only its id and title", () => {
  assert.deepEqual(threadOpenCommand({ threadId: "thread-1", title: "First thread" }), {
    command: OPEN_THREAD_COMMAND,
    title: "Open Thread",
    arguments: ["thread-1", "First thread"],
  });
});

test("maps loading, pairing, empty, runtime unavailable, and connection failure states", async () => {
  let approve: (() => void) | undefined;
  let resolveConnection: ((connection: AttachConnection) => void) | undefined;
  const pending = new Promise<AttachConnection>((resolve) => { resolveConnection = resolve; });
  const model = new ThreadsModel(async (onPairingPending) => {
    approve = onPairingPending;
    return pending;
  });
  const states: string[] = [];
  model.onDidChange((state) => states.push(state.kind));

  const refresh = model.refresh();
  assert.equal(stateKind(model), "loading");
  approve!();
  assert.equal(stateKind(model), "pairing");
  resolveConnection!(new FakeConnection({ threads: [] }));
  await refresh;
  assert.equal(stateKind(model), "empty");
  assert.deepEqual(states, ["loading", "pairing", "empty"]);
  model.dispose();

  const runtime = new ThreadsModel(async () => {
    throw new AttachTransportError("runtime_unavailable");
  });
  await runtime.refresh();
  assert.equal(runtime.state.kind, "runtime-unavailable");

  const failure = new ThreadsModel(async () => {
    throw new AttachTransportError("authorization_expired");
  });
  await failure.refresh();
  assert.equal(failure.state.kind, "connection-failed");
});

test("refresh disposes the old attach connection and replaces its results", async () => {
  const first = new FakeConnection({
    threads: [{ threadId: "old", title: "Old", updatedAt: "old-time" }],
  });
  const second = new FakeConnection({
    threads: [{ threadId: "new", title: "New", updatedAt: "new-time" }],
  });
  const model = new ThreadsModel(connectorFor(first, second), (value) => value);

  await model.refresh();
  await model.refresh();

  assert.equal(first.disposed, true);
  assert.equal(second.disposed, false);
  assert.deepEqual(model.state, {
    kind: "ready",
    threads: [{ threadId: "new", title: "New", description: "new-time" }],
  });
  assert.deepEqual(first.listCalls, [undefined]);
  assert.deepEqual(second.listCalls, [undefined]);
  model.dispose();
});

test("dispose closes active and late attach connections and stops updates", async () => {
  const active = new FakeConnection({ threads: [] });
  const model = new ThreadsModel(connectorFor(active));
  await model.refresh();
  model.dispose();
  assert.equal(active.disposed, true);

  let resolveConnection: ((connection: AttachConnection) => void) | undefined;
  const pendingModel = new ThreadsModel(() => new Promise((resolve) => { resolveConnection = resolve; }));
  const refresh = pendingModel.refresh();
  pendingModel.dispose();
  const late = new FakeConnection({ threads: [] });
  resolveConnection!(late);
  await refresh;
  assert.equal(late.disposed, true);
  assert.equal(pendingModel.state.kind, "loading");
});

test("submits a nonblank run once without context and reports acceptance", async () => {
  const connection = new FakeConnection({ threads: [] });
  const model = new ThreadsModel(connectorFor(connection));
  await model.refresh();

  assert.deepEqual(await model.submitRun("Summarize this change."), {
    kind: "accepted", runId: "run-1", committedSeq: 1, acceptedAt: "2026-07-18T12:00:00Z",
  });
  assert.deepEqual(connection.startCalls, [{ text: "Summarize this change.", context: undefined }]);
  model.dispose();
});

test("subscribes from the accepted run cursor without changing it", async () => {
  const connection = new FakeConnection({ threads: [] });
  const subscription = { dispose() {} } as RunStreamSubscription;
  connection.streamResult = Promise.resolve(subscription);
  const model = new ThreadsModel(connectorFor(connection));
  await model.refresh();

  assert.equal(await model.streamRun("run-1", 7), subscription);
  assert.deepEqual(connection.streamCalls, [{ runId: "run-1", afterRunSeq: 7 }]);
  model.dispose();
});

test("treats canceled and blank run submissions as no-ops", async () => {
  const connection = new FakeConnection({ threads: [] });
  const model = new ThreadsModel(connectorFor(connection));
  await model.refresh();

  assert.deepEqual(await model.submitRun(undefined), { kind: "no-op" });
  assert.deepEqual(await model.submitRun("  \n\t"), { kind: "no-op" });
  assert.deepEqual(connection.startCalls, []);
  model.dispose();
});

test("reports an unavailable connection without attempting a run", async () => {
  const model = new ThreadsModel(async () => { throw new AttachTransportError("runtime_unavailable"); });
  await model.refresh();

  assert.deepEqual(await model.submitRun("Start it"), { kind: "unavailable" });
  model.dispose();
});

test("maps run failures to sanitized actionable messages", async () => {
  const connection = new FakeConnection({ threads: [] });
  connection.startResult = Promise.reject(new AttachTransportError("authorization_expired"));
  const model = new ThreadsModel(connectorFor(connection));
  await model.refresh();

  assert.deepEqual(await model.submitRun("Start it"), {
    kind: "failed",
    message: "Muniment authorization expired. Refresh Threads and try again.",
  });
  connection.startResult = Promise.reject(new Error("secret capability and protocol payload"));
  assert.deepEqual(await model.submitRun("Try again"), {
    kind: "failed",
    message: "Muniment couldn’t start the run. Try again.",
  });
  model.dispose();
});

test("guards against concurrent run submissions while one is pending", async () => {
  let accept: ((value: RunStartAccepted) => void) | undefined;
  const connection = new FakeConnection({ threads: [] });
  connection.startResult = new Promise((resolve) => { accept = resolve; });
  const model = new ThreadsModel(connectorFor(connection));
  await model.refresh();

  const first = model.submitRun("First");
  assert.deepEqual(await model.submitRun("Second"), { kind: "busy" });
  assert.deepEqual(connection.startCalls, [{ text: "First", context: undefined }]);
  accept!({ runId: "run-1", committedSeq: 1, acceptedAt: "2026-07-18T12:00:00Z" });
  assert.deepEqual(await first, {
    kind: "accepted", runId: "run-1", committedSeq: 1, acceptedAt: "2026-07-18T12:00:00Z",
  });
  model.dispose();
});
