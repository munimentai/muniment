import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";
import { ATTACH_PROTOCOL, decodeAttachEnvelope, decodeAttachJson } from "../src/protocol";

const fixtureDirectory = path.resolve(__dirname, "../../../protocol-fixtures/muniment.attach/1");
const fixtureNames = readdirSync(fixtureDirectory).filter((name) => name.endsWith(".json")).sort();

test("decodes every canonical fixture directly from the Rust-owned corpus", () => {
  assert.ok(fixtureNames.length > 0, "fixture corpus must not be empty");
  const kinds = new Set<string>();
  for (const name of fixtureNames) {
    const bytes = readFileSync(path.join(fixtureDirectory, name), "utf8");
    kinds.add(decodeAttachJson(bytes).kind);
  }
  assert.deepEqual(
    [...kinds].sort(),
    ["authorization", "error", "event", "negotiation", "request", "response"],
  );
});

test("rejects an incompatible protocol version", () => {
  const request = JSON.parse(
    readFileSync(path.join(fixtureDirectory, "request-thread-list.json"), "utf8"),
  ) as Record<string, unknown>;
  request.protocol = "muniment.attach/2";
  assert.throws(() => decodeAttachEnvelope(request), /incompatible attach protocol/);
});

test("accepts unknown optional fields", () => {
  const request = JSON.parse(
    readFileSync(path.join(fixtureDirectory, "request-thread-list.json"), "utf8"),
  ) as Record<string, unknown>;
  request.future_optional = { enabled: true };
  const decoded = decodeAttachEnvelope(request);
  assert.equal(decoded.kind, "request");
  assert.deepEqual(decoded.future_optional, { enabled: true });
});

test("fails closed for malformed and unrecognized input", () => {
  assert.throws(() => decodeAttachJson("{"), /not valid JSON/);
  assert.throws(() => decodeAttachEnvelope(null), /JSON object/);
  assert.throws(() => decodeAttachEnvelope({ protocol: ATTACH_PROTOCOL }), /unrecognized/);
  assert.throws(
    () => decodeAttachEnvelope({ protocol: ATTACH_PROTOCOL, request_id: "id", ok: true }),
    /unrecognized/,
  );
  assert.throws(
    () => decodeAttachEnvelope({
      protocol: ATTACH_PROTOCOL,
      client: { kind: "editor-extension", version: "0.0.1" },
      supported: { min: 1.5, max: 2 },
      client_nonce: "nonce",
    }),
    /invalid attach hello/,
  );
});

test("rejects conflicting envelope shapes", () => {
  const base = { protocol: ATTACH_PROTOCOL };
  const request = {
    ...base, request_id: "id", operation: "thread.list", capability: "capability", body: {},
  };

  assert.throws(() => decodeAttachEnvelope({ ...request, ok: true }), /conflicting/);
  assert.throws(() => decodeAttachEnvelope({ ...request, desktop_version: "0.0.1" }), /conflicting/);
  assert.throws(
    () => decodeAttachEnvelope({ ...base, request_id: "id", ok: true, body: {}, error: {
      code: "invalid_request", message: "invalid", retryable: false,
    } }),
    /unrecognized/,
  );
  assert.throws(
    () => decodeAttachEnvelope({ ...base, request_id: "id", ok: false, body: {}, error: {
      code: "invalid_request", message: "invalid", retryable: false,
    } }),
    /unrecognized/,
  );
  assert.throws(
    () => decodeAttachEnvelope({
      capability: "capability", expires_at: 3600, idle_timeout_seconds: 900,
      authorized_client_credential: "credential",
      workspace_scopes: {}, operation: "thread.list", request_id: "id", body: {},
    }),
    /conflicting/,
  );
  assert.throws(
    () => decodeAttachEnvelope({
      ...base, subscription_id: "subscription", event: "run.event", request_id: "id", body: {},
    }),
    /conflicting/,
  );
  assert.throws(
    () => decodeAttachEnvelope({
      ...base, client: { kind: "editor-extension", version: "0.0.1" },
      supported: { min: 1, max: 1 }, client_nonce: "nonce", ok: true,
    }),
    /conflicting/,
  );
});
