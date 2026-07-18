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
