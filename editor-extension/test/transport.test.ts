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
  await assert.rejects(connectAttach({ clientVersion: "1", platform: "win32" }),
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
