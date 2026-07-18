import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import test from "node:test";
import {
  AttachFrameDecoder,
  AttachTransportError,
  MAX_FRAME_LENGTH,
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
const authorized = {
  capability: "c".repeat(64),
  expires_at: 3600,
  idle_timeout_seconds: 900,
  workspace_scopes: {},
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

test("starts runs with the canonical request shape and decodes the receipt", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);

  const started = connection.startRun("Summarize the selected file.", { selected_file: "src/main.rs" });
  const request = lastRequest(socket);
  assert.equal(request.operation, "run.start");
  assert.equal(request.capability, "c".repeat(64));
  assert.deepEqual(request.body, {
    context: { selected_file: "src/main.rs" }, text: "Summarize the selected file.",
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

test("rejects invalid inputs before writing and bounds concurrent requests", async () => {
  const socket = new FakeSocket();
  const connection = await authorizedConnection(socket);
  const writes = socket.writes.length;
  await assert.rejects(connection.listThreads(""), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "unexpected_message");
  await assert.rejects(connection.openThread(""), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "unexpected_message");
  assert.equal(socket.writes.length, writes);

  const first = connection.listThreads();
  await assert.rejects(connection.startRun("prompt"), (error: unknown) =>
    error instanceof AttachTransportError && error.code === "unexpected_message");
  const request = lastRequest(socket);
  socket.emit("data", encodeAttachFrame({ protocol: "muniment.attach/1",
    request_id: request.request_id, ok: true, body: { threads: [] } }));
  assert.deepEqual(await first, { threads: [] });
  connection.dispose();
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
