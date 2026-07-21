import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";
import { RunDocument } from "../src/run-document";
import {
  AttachFrameDecoder,
  AttachTransportError,
  MAX_FRAME_LENGTH,
  MAX_PENDING_REQUESTS,
  connectAttach,
  encodeAttachFrame,
} from "../src/transport";

const welcome = {
  selected: 1,
  desktop_version: "0.0.1",
  server_nonce: "a".repeat(32),
  authorization: "pairing_required",
  approval_challenge: "b".repeat(32),
};
const reconnectWelcome = { ...welcome, authorization: "authorized" };
const authorized = {
  capability: "c".repeat(64),
  expires_at: 3600,
  idle_timeout_seconds: 900,
  workspace_scopes: {},
  authorized_client_credential: "d".repeat(64),
};

class FakeSocket extends EventEmitter {
  writes: Buffer[] = [];
  destroyed = false;

  write(bytes: Uint8Array): boolean {
    this.writes.push(Buffer.from(bytes));
    return true;
  }

  destroy(): this {
    this.destroyed = true;
    return this;
  }
}

function connecting(socket: FakeSocket, overrides: Record<string, unknown> = {}) {
  return connectAttach({
    clientVersion: "0.0.1",
    platform: "linux",
    environment: { XDG_RUNTIME_DIR: "/run/user/1000" },
    createSocket: (endpoint) => {
      assert.equal(endpoint, "/run/user/1000/muniment/attach-v1.sock");
      return socket;
    },
    nonce: () => "d".repeat(32),
    ioTimeoutMs: 50,
    approvalTimeoutMs: 50,
    ...overrides,
  });
}

async function authorizedConnection(socket: FakeSocket) {
  const result = connecting(socket);
  socket.emit("connect");
  socket.emit("data", Buffer.concat([encodeAttachFrame(welcome), encodeAttachFrame(authorized)]));
  return result;
}

function lastRequest(socket: FakeSocket) {
  return new AttachFrameDecoder().push(socket.writes.at(-1)!)[0];
}

test("pairs across fragmented frames and sends the editor-extension hello", async () => {
  const socket = new FakeSocket();
  let pending = 0;
  const result = connecting(socket, { onPairingPending: () => pending++ });
  socket.emit("connect");
  const hello = new AttachFrameDecoder().push(socket.writes[0])[0];
  assert.equal((hello.client as Record<string, unknown>).kind, "editor-extension");
  assert.equal(hello.client_nonce, "d".repeat(32));

  const welcomeFrame = encodeAttachFrame(welcome);
  for (const byte of welcomeFrame) socket.emit("data", Buffer.of(byte));
  const authorizationFrame = encodeAttachFrame(authorized);
  socket.emit("data", authorizationFrame.subarray(0, 7));
  socket.emit("data", authorizationFrame.subarray(7));

  const connection = await result;
  assert.equal(pending, 1);
  assert.equal(connection.capability, "c".repeat(64));
  connection.dispose();
  assert.equal(connection.capability, "");
  assert.equal(socket.destroyed, true);
  assert.equal(socket.listenerCount("data"), 0);
});

test("accepts coalesced welcome and authorization frames", async () => {
  const socket = new FakeSocket();
  const result = connecting(socket);
  socket.emit("connect");
  socket.emit("data", Buffer.concat([encodeAttachFrame(welcome), encodeAttachFrame(authorized)]));
  assert.equal((await result).expiresInSeconds, 3600);
});

test("reconnect authorization does not surface pairing", async () => {
  const socket = new FakeSocket();
  let pending = 0;
  const result = connecting(socket, {
    authorizedClientCredential: "d".repeat(64),
    onPairingPending: () => pending++,
  });
  socket.emit("connect");
  socket.emit("data", Buffer.concat([
    encodeAttachFrame(reconnectWelcome), encodeAttachFrame(authorized),
  ]));
  assert.equal((await result).capability, "c".repeat(64));
  assert.equal(pending, 0);
});

test("invalid reconnect surfaces pairing and cannot authorize without approval", async () => {
  const socket = new FakeSocket();
  let pending = 0;
  const result = connecting(socket, {
    authorizedClientCredential: "e".repeat(64),
    onPairingPending: () => pending++,
  });
  socket.emit("connect");
  socket.emit("data", encodeAttachFrame(welcome));
  await assert.rejects(result, (error: unknown) =>
    error instanceof AttachTransportError && error.code === "timeout");
  assert.equal(pending, 1);
});

test("lists and opens bounded thread pages over the authorized connection", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);

  const listed = connection.listThreads("opaque-list-cursor");
  const listRequest = lastRequest(socket);
  assert.equal(listRequest.operation, "thread.list");
  assert.deepEqual(listRequest.body, { limit: 100, cursor: "opaque-list-cursor" });
  assert.equal(listRequest.capability, "c".repeat(64));
  socket.emit("data", encodeAttachFrame({
    protocol: "muniment.attach/1", request_id: listRequest.request_id, ok: true,
    body: { threads: [{ thread_id: "thread-1", title: "First", updated_at: "2026-07-18T00:00:00Z" }],
      next_cursor: "opaque-next" },
  }));
  assert.deepEqual(await listed, { threads: [{ threadId: "thread-1", title: "First",
    updatedAt: "2026-07-18T00:00:00Z" }], nextCursor: "opaque-next" });

  const opened = connection.openThread("thread-1");
  const openRequest = lastRequest(socket);
  assert.equal(openRequest.operation, "thread.open");
  assert.deepEqual(openRequest.body, { thread_id: "thread-1", limit: 100 });
  socket.emit("data", encodeAttachFrame({
    protocol: "muniment.attach/1", request_id: openRequest.request_id, ok: true,
    body: { thread_id: "thread-1", entries: [{ run_seq: 1, kind: "message", text: "hello" }] },
  }));
  assert.deepEqual(await opened, { threadId: "thread-1",
    entries: [{ runSeq: 1, kind: "message", text: "hello" }] });
  connection.dispose();
});

test("onboards workspace context and lazily ensures Home over the authorized connection", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);

  const onboarding = connection.onboardWorkspace("/work/repo", "/external/memory");
  const onboardRequest = lastRequest(socket);
  assert.equal(onboardRequest.operation, "workspace.onboard");
  assert.deepEqual(onboardRequest.body, {
    opened_directory: "/work/repo", memory_location: "/external/memory",
  });
  socket.emit("data", encodeAttachFrame({
    protocol: "muniment.attach/1", request_id: onboardRequest.request_id, ok: true,
    body: { opened_directory: "/work/repo", memory_location: "/external/memory",
      instructions: "nearest instructions" },
  }));
  assert.deepEqual(await onboarding, { openedDirectory: "/work/repo",
    memoryLocation: "/external/memory", instructions: "nearest instructions" });

  const ensuring = connection.ensureHome();
  const ensureRequest = lastRequest(socket);
  assert.equal(ensureRequest.operation, "home.ensure");
  assert.deepEqual(ensureRequest.body, {});
  socket.emit("data", encodeAttachFrame({
    protocol: "muniment.attach/1", request_id: ensureRequest.request_id, ok: true, body: {},
  }));
  await ensuring;
  connection.dispose();
});

test("starts runs with the canonical request shape and decodes the receipt", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);

  const started = connection.startRun("Summarize the selected file.", { selected_file: "src/main.rs" }, "/repo/root");
  const request = lastRequest(socket);
  assert.equal(request.operation, "run.start");
  assert.equal((request.body as Record<string, unknown>).workspace, "/repo/root");
  assert.equal(request.capability, "c".repeat(64));
  assert.deepEqual(request.body, {
    context: { selected_file: "src/main.rs" }, text: "Summarize the selected file.",
    workspace: "/repo/root",
  });
  assert.match(request.request_id as string, /^[0-9a-f-]{36}$/);
  assert.match(request.idempotency_key as string, /^[0-9a-f-]{36}$/);
  assert.notEqual(request.idempotency_key, request.request_id);
  socket.emit("data", encodeAttachFrame({
    protocol: "muniment.attach/1", request_id: request.request_id, ok: true,
    body: { accepted_at: "2026-07-17T00:00:00Z", committed_seq: 1,
      run_id: "00000000000000000000000000000191" },
  }));
  assert.deepEqual(await started, { acceptedAt: "2026-07-17T00:00:00Z", committedSeq: 1,
    runId: "00000000000000000000000000000191" });

  const second = connection.startRun("Another prompt");
  const secondRequest = lastRequest(socket);
  assert.deepEqual(secondRequest.body, { text: "Another prompt" });
  assert.notEqual(secondRequest.idempotency_key, request.idempotency_key);
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: secondRequest.request_id, ok: true,
    body: { run_id: "01900000-0000-7000-8000-000000000001", committed_seq: 2,
      accepted_at: "2026-07-17T00:00:00.123+05:30" } }));
  await second;
  connection.dispose();
});

test("delivers an ordered run stream while correlated requests and events interleave", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const runId = "00000000000000000000000000000191";
  const subscribing = connection.streamRun(runId, 0);
  const request = lastRequest(socket);
  assert.equal(request.operation, "run.stream");
  assert.deepEqual(request.body, {
    run_id: "00000000000000000000000000000191", after_run_seq: 0,
  });
  const subscriptionId = "00000000000000000000000000000190";
  socket.emit("data", Buffer.concat([
    encodeAttachFrame({ protocol: "muniment.attach/1", request_id: request.request_id, ok: true,
      body: { subscription_id: subscriptionId, run_id: runId,
        first_available_run_seq: 1, current_run_seq: 2,
        window: { max_events: 4, max_bytes: 4096 } } }),
    encodeAttachFrame({ protocol: "muniment.attach/1", subscription_id: subscriptionId,
      event: "run.event", run_id: runId, run_seq: 1,
      body: { event_type: "assistant.message", event_version: 1,
        recorded_at: "2026-07-17T00:00:00Z", payload: { withheld: true } } }),
  ]));
  const subscription = await subscribing;
  const delivered: unknown[] = [];
  const listener = subscription.onDidReceiveMessage((event) => delivered.push(event));

  const listing = connection.listThreads();
  const listRequest = lastRequest(socket);
  socket.emit("data", Buffer.concat([
    encodeAttachFrame({ protocol: "muniment.attach/1", subscription_id: subscriptionId,
      event: "run.event", run_id: runId, run_seq: 2,
      body: { event_type: "run.completed", event_version: 1,
        recorded_at: "2026-07-17T00:00:01Z", payload: { withheld: true } } }),
    encodeAttachFrame({ protocol: "muniment.attach/1", request_id: listRequest.request_id,
      ok: true, body: { threads: [] } }),
    encodeAttachFrame({ protocol: "muniment.attach/1", subscription_id: subscriptionId,
      event: "subscription.caught_up", run_id: runId, run_seq: 2, body: {} }),
    encodeAttachFrame({ protocol: "muniment.attach/1", subscription_id: subscriptionId,
      event: "run.event", run_id: runId, run_seq: 3,
      body: { event_type: "assistant.message", event_version: 1,
        recorded_at: "2026-07-17T00:00:02Z", payload: { withheld: true } } }),
  ]));
  assert.deepEqual(await listing, { threads: [] });
  assert.deepEqual(delivered.map((event: any) => [event.type, event.runSeq]), [
    ["run.event", 1], ["run.event", 2], ["subscription.caught_up", 2], ["run.event", 3],
  ]);

  const acknowledged = subscription.acknowledge(3);
  const ack = lastRequest(socket);
  assert.equal(ack.operation, "run.cursor_ack");
  assert.deepEqual(ack.body, { subscription_id: subscriptionId, through_run_seq: 3 });
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: ack.request_id, ok: true,
    body: { subscription_id: subscriptionId, through_run_seq: 3 } }));
  await acknowledged;
  listener.dispose();
  connection.dispose();
});

test("fails closed for invalid stream sequence and rejects acknowledgements after disposal", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const runId = "00000000000000000000000000000191";
  const subscribing = connection.streamRun(runId, 0);
  const request = lastRequest(socket);
  const subscriptionId = "00000000000000000000000000000190";
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: request.request_id, ok: true, body: { subscription_id: subscriptionId,
      run_id: runId, first_available_run_seq: 1, current_run_seq: 2,
      window: { max_events: 2, max_bytes: 4096 } } }));
  const subscription = await subscribing;
  await assert.rejects(subscription.acknowledge(1), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "unexpected_message");
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    subscription_id: subscriptionId, event: "run.event", run_id: runId,
    run_seq: 2, body: { event_type: "assistant.message", event_version: 1,
      recorded_at: "2026-07-17T00:00:00Z", payload: { withheld: true } } }));
  assert.equal(socket.destroyed, true);
  await assert.rejects(subscription.acknowledge(1), (error: unknown) =>
    error instanceof AttachTransportError);
});

test("atomically routes concurrent stream responses and coalesced terminal events", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const runA = "00000000000000000000000000000191";
  const runB = "00000000000000000000000000000192";
  const pendingA = connection.streamRun(runA, 0);
  const requestA = lastRequest(socket);
  const pendingB = connection.streamRun(runB, 0);
  const requestB = lastRequest(socket);
  const subscriptionA = "00000000000000000000000000000190";
  const subscriptionB = "00000000000000000000000000000193";
  const response = (requestId: unknown, subscriptionId: string, runId: string) =>
    encodeAttachFrame({ protocol: "muniment.attach/1", request_id: requestId, ok: true,
      body: { subscription_id: subscriptionId, run_id: runId, first_available_run_seq: 1,
        current_run_seq: 1, window: { max_events: 4, max_bytes: 4096 } } });
  const event = (subscriptionId: string, runId: string) => encodeAttachFrame({
    protocol: "muniment.attach/1", subscription_id: subscriptionId, event: "run.event",
    run_id: runId, run_seq: 1, body: { event_type: "run.completed", event_version: 1,
      recorded_at: "2026-07-17T00:00:00Z", payload: { withheld: true,
        receipt: { route: "cloud", capabilities: [{ name: "search", version: "1" }] } } },
  });
  const closed = (subscriptionId: string, runId: string) => encodeAttachFrame({
    protocol: "muniment.attach/1", subscription_id: subscriptionId, event: "stream.closed",
    run_id: runId, run_seq: 2, body: { code: "completed", resumable: false },
  });
  socket.emit("data", Buffer.concat([
    response(requestB.request_id, subscriptionB, runB), event(subscriptionB, runB),
    response(requestA.request_id, subscriptionA, runA), event(subscriptionA, runA),
    closed(subscriptionA, runA),
  ]));
  const [streamA, streamB] = await Promise.all([pendingA, pendingB]);
  const deliveredA: unknown[] = [];
  streamA.onDidReceiveMessage((message) => deliveredA.push(message));
  assert.deepEqual(deliveredA.map((message: any) => message.type), ["run.event", "stream.closed"]);
  assert.deepEqual((deliveredA[0] as any).payload.receipt,
    { route: "cloud", capabilities: [{ name: "search", version: "1" }] });
  await assert.rejects(streamA.acknowledge(1), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "unexpected_message");
  const deliveredB: unknown[] = [];
  streamB.onDidReceiveMessage((message) => deliveredB.push(message));
  assert.equal(deliveredB.length, 1);
  connection.dispose();
});

test("delivers bounded pending permissions while allow and deny answers are interleaved", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const runId = "01900000-0000-7000-8000-000000000001";
  const subscriptionId = "01900000-0000-7000-8000-000000000002";
  const subscribing = connection.streamRun(runId, 0);
  const streamRequest = lastRequest(socket);
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: streamRequest.request_id, ok: true, body: { subscription_id: subscriptionId,
      run_id: runId, first_available_run_seq: 1, current_run_seq: 2,
      window: { max_events: 4, max_bytes: 4096 } } }));
  const stream = await subscribing;
  const delivered: unknown[] = [];
  stream.onDidReceiveMessage((message) => delivered.push(message));

  const allowing = connection.answerPermission(runId, "secret-gate", "allow");
  const allowRequest = lastRequest(socket);
  assert.equal(allowRequest.operation, "permission.answer");
  assert.deepEqual(allowRequest.body,
    { run_id: runId, gate_id: "secret-gate", decision: "allow" });
  assert.match(allowRequest.request_id as string, /^[0-9a-f-]{36}$/);
  assert.match(allowRequest.idempotency_key as string, /^[0-9a-f-]{36}$/);
  assert.notEqual(allowRequest.request_id, allowRequest.idempotency_key);
  socket.emit("data", Buffer.concat([
    encodeAttachFrame({ protocol: "muniment.attach/1", subscription_id: subscriptionId,
      event: "permission.pending", run_id: runId, run_seq: 1,
      body: { gate_id: "next-secret", kind: "confirm", title: "Allow access?",
        message: "Sensitive details" } }),
    encodeAttachFrame({ protocol: "muniment.attach/1", request_id: allowRequest.request_id,
      ok: true, body: { run_id: runId, gate_id: "secret-gate", decision: "allow",
        committed_seq: 2, accepted_at: "2026-07-17T00:00:00Z" } }),
  ]));
  assert.deepEqual(await allowing, { runId, gateId: "secret-gate", decision: "allow",
    committedSeq: 2, acceptedAt: "2026-07-17T00:00:00Z" });
  assert.deepEqual(delivered, [{ type: "permission.pending", runSeq: 1,
    gateId: "next-secret", kind: "confirm", title: "Allow access?",
    message: "Sensitive details" }]);

  const denying = connection.answerPermission(runId, "next-secret", "deny");
  const denyRequest = lastRequest(socket);
  assert.notEqual(denyRequest.request_id, allowRequest.request_id);
  assert.notEqual(denyRequest.idempotency_key, allowRequest.idempotency_key);
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: denyRequest.request_id, ok: true, body: { run_id: runId,
      gate_id: "next-secret", decision: "deny", committed_seq: 3,
      accepted_at: "2026-07-17T00:00:01Z" } }));
  assert.equal((await denying).decision, "deny");
  connection.dispose();
});

test("fails closed on hostile permission events and receipts without leaking gate content", async (t) => {
  const hostileBodies = [
    { gate_id: "gate", kind: "other", title: "Title" },
    { gate_id: " ", kind: "confirm", title: "Title" },
    { gate_id: "x".repeat(257), kind: "confirm", title: "Title" },
    { gate_id: "gate", kind: "confirm", title: " " },
    { gate_id: "gate", kind: "confirm", title: "x".repeat(1025) },
    { gate_id: "gate", kind: "confirm", title: "Title", message: "x".repeat(4097) },
    { gate_id: "gate", kind: "confirm", title: "Title", payload: "unapproved" },
  ];
  for (const [index, body] of hostileBodies.entries()) await t.test(`event ${index}`, async () => {
    const socket = new FakeSocket();
    const connection = await authorizedConnection(socket);
    const runId = "01900000-0000-7000-8000-000000000001";
    const subscribing = connection.streamRun(runId, 0);
    const request = lastRequest(socket);
    socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
      request_id: request.request_id, ok: true, body: {
        subscription_id: "sub", run_id: runId, first_available_run_seq: 1,
        current_run_seq: 1, window: { max_events: 4, max_bytes: 4096 } } }));
    await subscribing;
    socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
      subscription_id: "sub", event: "permission.pending", run_id: runId, run_seq: 1, body }));
    assert.equal(socket.destroyed, true);
  });

  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const runId = "01900000-0000-7000-8000-000000000001";
  const writesBeforeInvalid = socket.writes.length;
  await assert.rejects(connection.answerPermission("bad-run", "gate", "allow"),
    (error: unknown) => error instanceof AttachTransportError && error.code === "unexpected_message");
  await assert.rejects(connection.answerPermission(runId, " ", "allow"),
    (error: unknown) => error instanceof AttachTransportError && error.code === "unexpected_message");
  await assert.rejects(connection.answerPermission(runId, "gate", "invalid" as any),
    (error: unknown) => error instanceof AttachTransportError && error.code === "unexpected_message");
  assert.equal(socket.writes.length, writesBeforeInvalid);
  const answer = connection.answerPermission(runId, "top-secret-gate", "allow");
  const request = lastRequest(socket);
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: request.request_id, ok: false,
    error: { code: "persistence_failed", message: "top-secret-gate details", retryable: false } }));
  await assert.rejects(answer, (error: unknown) => error instanceof AttachTransportError &&
    error.code === "desktop_failed" && !error.message.includes("top-secret-gate"));

  const hostile = connection.answerPermission(runId, "another-secret", "deny");
  const hostileRequest = lastRequest(socket);
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: hostileRequest.request_id, ok: true, body: { run_id: runId,
      gate_id: "wrong-secret", decision: "deny", committed_seq: 2,
      accepted_at: "2026-07-17T00:00:00Z" } }));
  await assert.rejects(hostile, (error: unknown) => error instanceof AttachTransportError &&
    error.code === "unexpected_message" && !error.message.includes("secret"));
});

test("projects a same-cursor resumable close without losing the terminal event", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const runId = "00000000000000000000000000000191";
  const pendingStream = connection.streamRun(runId, 0);
  const streamRequest = lastRequest(socket);
  const subscriptionId = "00000000000000000000000000000190";
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: streamRequest.request_id, ok: true, body: { subscription_id: subscriptionId,
      run_id: runId, first_available_run_seq: 1, current_run_seq: 1,
      window: { max_events: 4, max_bytes: 4096 } } }));
  const document = new RunDocument(runId, 0);
  await document.attach(pendingStream);

  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    subscription_id: subscriptionId, event: "run.event", run_id: runId, run_seq: 1,
    body: { event_type: "run.failed", event_version: 1,
      recorded_at: "2026-07-17T00:00:00Z", payload: { withheld: true } } }));
  await new Promise<void>((resolve) => setImmediate(resolve));
  const acknowledgement = lastRequest(socket);
  assert.equal(acknowledgement.operation, "run.cursor_ack");

  socket.emit("data", Buffer.concat([
    encodeAttachFrame({ protocol: "muniment.attach/1", request_id: acknowledgement.request_id,
      ok: false, error: { code: "invalid_cursor", message: "private details", retryable: true } }),
    encodeAttachFrame({ protocol: "muniment.attach/1", subscription_id: subscriptionId,
      event: "stream.closed", run_id: runId, run_seq: 1,
      body: { code: "invalid_cursor", resumable: true } }),
  ]));
  await new Promise<void>((resolve) => setImmediate(resolve));

  assert.match(document.content, /Status:\*\* Failed · Refresh Threads to retry\./);
  assert.match(document.content, /Final delivery acknowledgement failed/);
  assert.doesNotMatch(document.content, /private details|invalid_cursor|Stream closed/);
  assert.equal(socket.destroyed, false);
  document.dispose();
  connection.dispose();
});

test("subscription disposal rejects its acknowledgement and ignores the late response", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const runId = "00000000000000000000000000000191";
  const pendingStream = connection.streamRun(runId, 0);
  const streamRequest = lastRequest(socket);
  const subscriptionId = "00000000000000000000000000000190";
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: streamRequest.request_id, ok: true, body: { subscription_id: subscriptionId,
      run_id: runId, first_available_run_seq: 1, current_run_seq: 1,
      window: { max_events: 4, max_bytes: 4096 } } }));
  const stream = await pendingStream;
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    subscription_id: subscriptionId, event: "run.event", run_id: runId, run_seq: 1,
    body: { event_type: "assistant.message", event_version: 1,
      recorded_at: "2026-07-17T00:00:00Z", payload: { withheld: true } } }));
  await assert.rejects(stream.acknowledge(2), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "unexpected_message");
  const acknowledging = stream.acknowledge(1);
  const ackRequest = lastRequest(socket);
  stream.dispose();
  await assert.rejects(acknowledging, (error: unknown) =>
    error instanceof AttachTransportError && error.code === "connection_closed");
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: ackRequest.request_id, ok: true,
    body: { subscription_id: subscriptionId, through_run_seq: 1 } }));
  assert.equal(socket.destroyed, false);
  connection.dispose();
});

test("fails closed on stream regressions, mismatches, and hostile receipts", async (t) => {
  const invalidEvents = [
    { label: "regression", subscriptionId: "00000000000000000000000000000190",
      runId: "00000000000000000000000000000191", runSeq: 1,
      eventType: "assistant.message", payload: { withheld: true } },
    { label: "run mismatch", subscriptionId: "00000000000000000000000000000190",
      runId: "00000000000000000000000000000192", runSeq: 2,
      eventType: "assistant.message", payload: { withheld: true } },
    { label: "subscription mismatch", subscriptionId: "00000000000000000000000000000193",
      runId: "00000000000000000000000000000191", runSeq: 2,
      eventType: "assistant.message", payload: { withheld: true } },
    { label: "receipt on non-completed event", subscriptionId: "00000000000000000000000000000190",
      runId: "00000000000000000000000000000191", runSeq: 2,
      eventType: "assistant.message", payload: { withheld: true,
        receipt: { capabilities: [] } } },
    { label: "unbounded receipt capability", subscriptionId: "00000000000000000000000000000190",
      runId: "00000000000000000000000000000191", runSeq: 2,
      eventType: "run.completed", payload: { withheld: true,
        receipt: { capabilities: [{ name: "x".repeat(1025), version: "1" }] } } },
  ];
  for (const invalid of invalidEvents) await t.test(invalid.label, async () => {
    const socket = new FakeSocket();
    const connection = await authorizedConnection(socket);
    const streamPending = connection.streamRun("00000000000000000000000000000191", 0);
    const request = lastRequest(socket);
    socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
      request_id: request.request_id, ok: true,
      body: { subscription_id: "00000000000000000000000000000190",
        run_id: "00000000000000000000000000000191", first_available_run_seq: 1,
        current_run_seq: 2, window: { max_events: 4, max_bytes: 4096 } } }));
    await streamPending;
    socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
      subscription_id: "00000000000000000000000000000190", event: "run.event",
      run_id: "00000000000000000000000000000191", run_seq: 1,
      body: { event_type: "assistant.message", event_version: 1,
        recorded_at: "2026-07-17T00:00:00Z", payload: { withheld: true } } }));
    socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
      subscription_id: invalid.subscriptionId, event: "run.event", run_id: invalid.runId,
      run_seq: invalid.runSeq, body: { event_type: invalid.eventType, event_version: 1,
        recorded_at: "2026-07-17T00:00:01Z", payload: invalid.payload } }));
    assert.equal(socket.destroyed, true);
  });
});

test("isolates throwing stream listeners from the connection and other listeners", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const runId = "00000000000000000000000000000191";
  const pendingStream = connection.streamRun(runId, 0);
  const request = lastRequest(socket);
  const subscriptionId = "00000000000000000000000000000190";
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1", request_id: request.request_id,
    ok: true, body: { subscription_id: subscriptionId, run_id: runId,
      first_available_run_seq: 1, current_run_seq: 1,
      window: { max_events: 4, max_bytes: 4096 } } }));
  const stream = await pendingStream;
  stream.onDidReceiveMessage(() => { throw new Error("consumer failure"); });
  const delivered: unknown[] = [];
  stream.onDidReceiveMessage((message) => delivered.push(message));
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    subscription_id: subscriptionId, event: "run.event", run_id: runId, run_seq: 1,
    body: { event_type: "assistant.message", event_version: 1,
      recorded_at: "2026-07-17T00:00:00Z", payload: { withheld: true } } }));
  assert.equal(delivered.length, 1);
  assert.equal(socket.destroyed, false);
  connection.dispose();
});

test("connection disposal rejects pending work and removes stream listeners", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const pending = connection.listThreads();
  connection.dispose();
  await assert.rejects(pending, (error: unknown) => error instanceof AttachTransportError &&
    error.code === "connection_closed");
  await assert.rejects(connection.streamRun("00000000000000000000000000000191", 0),
    (error: unknown) => error instanceof AttachTransportError &&
      error.code === "authorization_expired");
  assert.equal(socket.listenerCount("data"), 0);
});

test("rejects invalid run-start inputs before writing", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const writes = socket.writes.length;
  for (const [text, context] of [
    [" \n\t", undefined],
    ["x".repeat(32 * 1024 + 1), undefined],
    ["prompt", "x".repeat(64 * 1024)],
    ["prompt", { invalid: Number.NaN }],
  ] as const) {
    await assert.rejects(connection.startRun(text, context), (error: unknown) =>
      error instanceof AttachTransportError && error.code === "unexpected_message");
  }
  assert.equal(socket.writes.length, writes);
  connection.dispose();
});

test("fails closed on excessively nested run-start context before writing", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const writes = socket.writes.length;
  let context: unknown = null;
  for (let depth = 0; depth < 20_000; depth++) context = [context];

  await assert.rejects(connection.startRun("prompt", context as never), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "unexpected_message" &&
    error.message === "unexpected_message");
  assert.equal(socket.writes.length, writes);
  connection.dispose();
});

test("fails closed while snapshotting stateful run-start context without closing", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const writes = socket.writes.length;
  let accesses = 0;
  const context = Object.defineProperty({}, "selected_file", {
    enumerable: true,
    get() {
      if (++accesses > 1) throw new Error("private getter detail");
      return "src/main.rs";
    },
  });

  await assert.rejects(connection.startRun("prompt", context), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "unexpected_message" &&
    error.message === "unexpected_message");
  assert.equal(socket.writes.length, writes);
  assert.equal(socket.destroyed, false);

  const listed = connection.listThreads();
  const request = lastRequest(socket);
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: request.request_id, ok: true, body: { threads: [] } }));
  assert.deepEqual(await listed, { threads: [] });
  connection.dispose();
});

test("fails closed on malformed run-start receipts", async () => {
  const invalidBodies = [
    { run_id: "bad", committed_seq: 1, accepted_at: "2026-07-17T00:00:00Z" },
    { run_id: "00000000000000000000000000000191", committed_seq: 0,
      accepted_at: "2026-07-17T00:00:00Z" },
    { run_id: "00000000000000000000000000000191", committed_seq: 1,
      accepted_at: "2026-02-30T00:00:00Z" },
    { run_id: "00000000000000000000000000000191", committed_seq: 1,
      accepted_at: "2026-07-17T00:00:00Z", detail: "private" },
  ];
  for (const body of invalidBodies) {
    const socket = new FakeSocket();
    const connection = await authorizedConnection(socket);
    const started = connection.startRun("prompt");
    const request = lastRequest(socket);
    socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
      request_id: request.request_id, ok: true, body }));
    await assert.rejects(started, (error: unknown) => error instanceof AttachTransportError &&
      error.code === "unexpected_message" && !error.message.includes("private"));
    connection.dispose();
  }
});

test("rejects invalid inputs before writing and correlates concurrent requests", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const writes = socket.writes.length;
  await assert.rejects(connection.listThreads(""), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "unexpected_message");
  await assert.rejects(connection.openThread(""), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "unexpected_message");
  assert.equal(socket.writes.length, writes);

  const first = connection.listThreads();
  const firstRequest = lastRequest(socket);
  const second = connection.startRun("prompt");
  const secondRequest = lastRequest(socket);
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: secondRequest.request_id, ok: true, body: {
      run_id: "00000000000000000000000000000191", committed_seq: 1,
      accepted_at: "2026-07-17T00:00:00Z",
    } }));
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: firstRequest.request_id, ok: true, body: { threads: [] } }));
  assert.deepEqual(await first, { threads: [] });
  assert.equal((await second).committedSeq, 1);
  connection.dispose();
});

test("bounds concurrent requests and releases capacity after a response", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const pending = Array.from({ length: MAX_PENDING_REQUESTS }, () => connection.listThreads());
  const requests = socket.writes.slice(-MAX_PENDING_REQUESTS)
    .map((frame) => new AttachFrameDecoder().push(frame)[0]);
  const writesAtLimit = socket.writes.length;

  await assert.rejects(connection.listThreads(), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "request_rejected");
  assert.equal(socket.writes.length, writesAtLimit);
  assert.equal(socket.destroyed, false);

  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: requests[0].request_id, ok: true, body: { threads: [] } }));
  assert.deepEqual(await pending[0], { threads: [] });
  const replacement = connection.listThreads();
  const replacementRequest = lastRequest(socket);
  assert.equal(socket.writes.length, writesAtLimit + 1);
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: replacementRequest.request_id, ok: true, body: { threads: [] } }));
  await replacement;
  connection.dispose();
  await Promise.allSettled(pending.slice(1));
});

test("bounds acknowledgement tombstones, ignores late responses, and expires them", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const runId = "00000000000000000000000000000191";
  const subscribing = connection.streamRun(runId, 0);
  const streamRequest = lastRequest(socket);
  const subscriptionId = "00000000000000000000000000000190";
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: streamRequest.request_id, ok: true, body: { subscription_id: subscriptionId,
      run_id: runId, first_available_run_seq: 1, current_run_seq: 1,
      window: { max_events: 4, max_bytes: 4096 } } }));
  const stream = await subscribing;
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    subscription_id: subscriptionId, event: "run.event", run_id: runId, run_seq: 1,
    body: { event_type: "assistant.message", event_version: 1,
      recorded_at: "2026-07-17T00:00:00Z", payload: { withheld: true } } }));
  const acknowledging = stream.acknowledge(1);
  const ackRequest = lastRequest(socket);
  stream.dispose();
  await assert.rejects(acknowledging, (error: unknown) =>
    error instanceof AttachTransportError && error.code === "connection_closed");

  const pending = Array.from({ length: MAX_PENDING_REQUESTS - 1 }, () => connection.listThreads());
  await assert.rejects(connection.listThreads(), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "request_rejected");
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: ackRequest.request_id, ok: true,
    body: { subscription_id: subscriptionId, through_run_seq: 1 } }));
  assert.equal(socket.destroyed, false);
  const afterLateResponse = connection.listThreads();
  assert.equal(lastRequest(socket).operation, "thread.list");

  connection.dispose();
  await Promise.allSettled([...pending, afterLateResponse]);

  const expirySocket = new FakeSocket();
  const expiryConnection = await authorizedConnection(expirySocket);
  const expiryStreamPending = expiryConnection.streamRun(runId, 0);
  const expiryStreamRequest = lastRequest(expirySocket);
  expirySocket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: expiryStreamRequest.request_id, ok: true, body: { subscription_id: subscriptionId,
      run_id: runId, first_available_run_seq: 1, current_run_seq: 1,
      window: { max_events: 4, max_bytes: 4096 } } }));
  const expiryStream = await expiryStreamPending;
  expirySocket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    subscription_id: subscriptionId, event: "run.event", run_id: runId, run_seq: 1,
    body: { event_type: "assistant.message", event_version: 1,
      recorded_at: "2026-07-17T00:00:00Z", payload: { withheld: true } } }));
  const expiryAck = expiryStream.acknowledge(1);
  expiryStream.dispose();
  await assert.rejects(expiryAck);
  await new Promise((resolve) => setTimeout(resolve, 60));
  const afterExpiry = Array.from({ length: MAX_PENDING_REQUESTS }, () => expiryConnection.listThreads());
  assert.equal(expirySocket.destroyed, false);
  expiryConnection.dispose();
  await Promise.allSettled(afterExpiry);
});

test("maps request errors without exposing server details", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const result = connection.listThreads();
  const request = lastRequest(socket);
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: request.request_id, ok: false,
    error: { code: "unauthorized", message: "private details", retryable: false } }));
  await assert.rejects(result, (error: unknown) => error instanceof AttachTransportError &&
    error.code === "authorization_expired" && !error.message.includes("private"));
  connection.dispose();
});

test("fails closed on stale correlations and malformed thread projections", async () => {
  const staleSocket = new FakeSocket();
  const staleConnection = await authorizedConnection(staleSocket);
  const stale = staleConnection.listThreads();
  staleSocket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: "stale", ok: true, body: { threads: [] } }));
  await assert.rejects(stale, (error: unknown) => error instanceof AttachTransportError &&
    error.code === "unexpected_message");
  assert.equal(staleSocket.destroyed, true);

  const malformedSocket = new FakeSocket();
  const malformedConnection = await authorizedConnection(malformedSocket);
  const malformed = malformedConnection.openThread("thread-1");
  const request = lastRequest(malformedSocket);
  malformedSocket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: request.request_id, ok: true,
    body: { thread_id: "other-thread", entries: [] } }));
  await assert.rejects(malformed, (error: unknown) => error instanceof AttachTransportError &&
    error.code === "unexpected_message");
  malformedConnection.dispose();
});

test("discovers the macOS endpoint and pairs across coalesced frames", async () => {
  const socket = new FakeSocket();
  let pending = 0;
  const result = connecting(socket, {
    platform: "darwin",
    environment: { XDG_RUNTIME_DIR: "/must/not/be/used" },
    homedir: () => "/Users/example",
    createSocket: (endpoint: string) => {
      assert.equal(endpoint, "/Users/example/Library/Application Support/Muniment/runtime/attach-v1.sock");
      return socket;
    },
    onPairingPending: () => pending++,
  });
  socket.emit("connect");
  socket.emit("data", Buffer.concat([encodeAttachFrame(welcome), encodeAttachFrame(authorized)]));

  const connection = await result;
  assert.equal(pending, 1);
  assert.equal(connection.capability, "c".repeat(64));
  connection.dispose();
});

test("derives the Windows pipe from the canonical SID bytes and completes pairing", async () => {
  const socket = new FakeSocket();
  let pending = 0;
  const result = connecting(socket, {
    platform: "win32",
    environment: { USERNAME: "must-not-be-used", USERPROFILE: "C:\\must-not-be-used" },
    homedir: () => { throw new Error("must not use home"); },
    discoverWindowsSid: () => "S-1-5-21-1111111111-2222222222-3333333333-1001",
    createSocket: (endpoint: string) => {
      assert.equal(endpoint, "\\\\.\\pipe\\Muniment\\attach-v1-44f5a322cb079e3ff5b7a4603370914b");
      return socket;
    },
    onPairingPending: () => pending++,
  });
  socket.emit("connect");
  socket.emit("data", Buffer.concat([encodeAttachFrame(welcome), encodeAttachFrame(authorized)]));

  const connection = await result;
  assert.equal(pending, 1);
  assert.equal(connection.capability, "c".repeat(64));
  connection.dispose();
  assert.equal(socket.destroyed, true);
});

test("frame decoder rejects oversize prefixes before receiving payload", () => {
  const prefix = Buffer.alloc(4);
  prefix.writeUInt32BE(MAX_FRAME_LENGTH + 1);
  assert.throws(() => new AttachFrameDecoder().push(prefix),
    (error: unknown) => error instanceof AttachTransportError && error.code === "payload_too_large");
});

test("malformed input fails closed and destroys the socket", async () => {
  const socket = new FakeSocket();
  const result = connecting(socket);
  socket.emit("connect");
  const malformed = Buffer.from("{");
  const frame = Buffer.alloc(5);
  frame.writeUInt32BE(1);
  malformed.copy(frame, 4);
  socket.emit("data", frame);
  await assert.rejects(result, (error: unknown) =>
    error instanceof AttachTransportError && error.code === "malformed_frame");
  assert.equal(socket.destroyed, true);
  assert.equal(socket.eventNames().length, 0);
});

test("maps incompatible protocol errors to the redacted vocabulary", async () => {
  const socket = new FakeSocket();
  const result = connecting(socket);
  socket.emit("connect");
  socket.emit("data", encodeAttachFrame({
    protocol: "muniment.attach/1",
    request_id: "request",
    ok: false,
    error: { code: "protocol_incompatible", message: "details", retryable: false },
  }));
  await assert.rejects(result, (error: unknown) =>
    error instanceof AttachTransportError && error.code === "protocol_incompatible" &&
    !error.message.includes("details"));
});

test("rejects an incompatible welcome selection", async () => {
  const socket = new FakeSocket();
  const result = connecting(socket);
  socket.emit("connect");
  socket.emit("data", encodeAttachFrame({ ...welcome, selected: 2 }));
  await assert.rejects(result, (error: unknown) =>
    error instanceof AttachTransportError && error.code === "protocol_incompatible");
});

test("uses a separate bounded approval timeout", async () => {
  const socket = new FakeSocket();
  const result = connecting(socket, { ioTimeoutMs: 100, approvalTimeoutMs: 5 });
  socket.emit("connect");
  socket.emit("data", encodeAttachFrame(welcome));
  await assert.rejects(result, (error: unknown) =>
    error instanceof AttachTransportError && error.code === "timeout");
  assert.equal(socket.destroyed, true);
  assert.equal(socket.eventNames().length, 0);
});

test("rejects unsupported platforms and invalid or unavailable runtime discovery", async () => {
  await assert.rejects(connectAttach({ clientVersion: "1", platform: "freebsd" }),
    (error: unknown) => error instanceof AttachTransportError && error.code === "unsupported_platform");
  await assert.rejects(connectAttach({ clientVersion: "1", platform: "linux", environment: {} }),
    (error: unknown) => error instanceof AttachTransportError && error.code === "runtime_unavailable");
  await assert.rejects(connectAttach({
    clientVersion: "1", platform: "linux", environment: { XDG_RUNTIME_DIR: "relative" },
  }), (error: unknown) => error instanceof AttachTransportError && error.code === "runtime_unavailable");
  await assert.rejects(connectAttach({
    clientVersion: "1", platform: "darwin", homedir: () => "relative",
  }), (error: unknown) => error instanceof AttachTransportError && error.code === "runtime_unavailable");
  await assert.rejects(connectAttach({
    clientVersion: "1", platform: "darwin", homedir: () => { throw new Error("unavailable"); },
  }), (error: unknown) => error instanceof AttachTransportError && error.code === "runtime_unavailable");
});

test("fails closed when Windows SID discovery is unavailable or malformed", async () => {
  let socketCreations = 0;
  const createSocket = () => {
    socketCreations++;
    return new FakeSocket();
  };
  await assert.rejects(connectAttach({
    clientVersion: "1",
    platform: "win32",
    environment: { USERNAME: "fallback-user", USERPROFILE: "C:\\fallback" },
    discoverWindowsSid: () => { throw new Error("private process output"); },
    createSocket,
  }), (error: unknown) => error instanceof AttachTransportError &&
    error.code === "runtime_unavailable" && error.message === "runtime_unavailable");
  await assert.rejects(connectAttach({
    clientVersion: "1",
    platform: "win32",
    discoverWindowsSid: () => "S-1-5-21-not-a-number",
    createSocket,
  }), (error: unknown) => error instanceof AttachTransportError && error.code === "runtime_unavailable");
  assert.equal(socketCreations, 0);
});
