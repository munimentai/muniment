export const ATTACH_PROTOCOL = "muniment.attach/1" as const;
const ATTACH_VERSION = 1;

type JsonObject = Record<string, unknown>;

export type AttachEnvelope =
  | (JsonObject & { kind: "negotiation"; phase: "hello" | "welcome" })
  | (JsonObject & { kind: "authorization" })
  | (JsonObject & { kind: "request" })
  | (JsonObject & { kind: "response" })
  | (JsonObject & { kind: "event" })
  | (JsonObject & { kind: "error" });

function isObject(value: unknown): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hasString(value: JsonObject, key: string): boolean {
  return typeof value[key] === "string" && (value[key] as string).length > 0;
}

function hasFiniteNumber(value: JsonObject, key: string): boolean {
  return typeof value[key] === "number" && Number.isFinite(value[key]);
}

function isProtocolVersion(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 1;
}

function requireProtocol(value: JsonObject): void {
  if (value.protocol !== ATTACH_PROTOCOL) {
    throw new Error(`incompatible attach protocol: ${String(value.protocol)}`);
  }
}

function hasAny(value: JsonObject, keys: readonly string[]): boolean {
  return keys.some((key) => key in value);
}

function rejectConflicts(value: JsonObject, keys: readonly string[]): void {
  if (hasAny(value, keys)) {
    throw new Error("conflicting attach envelope fields");
  }
}

const ENVELOPE_FIELDS = [
  "request_id", "operation", "capability", "idempotency_key", "ok", "error",
  "subscription_id", "event", "run_id", "run_seq", "body",
] as const;

const HANDSHAKE_FIELDS = [
  "protocol", "client", "supported", "client_nonce", "selected", "desktop_version",
  "server_nonce", "authorization", "approval_challenge", "expires_at",
  "idle_timeout_seconds", "workspace_scopes",
] as const;

const NON_ENVELOPE_SHAPE_FIELDS = HANDSHAKE_FIELDS.slice(1);

/** Decode one JSON value at the version-pinned language boundary. */
export function decodeAttachEnvelope(input: unknown): AttachEnvelope {
  if (!isObject(input)) {
    throw new Error("attach envelope must be a JSON object");
  }

  if ("client" in input || "supported" in input || "client_nonce" in input) {
    rejectConflicts(input, [...ENVELOPE_FIELDS, ...HANDSHAKE_FIELDS.slice(4)]);
    requireProtocol(input);
    const client = input.client;
    const supported = input.supported;
    if (!isObject(client) || !hasString(client, "kind") || !hasString(client, "version") ||
        !isObject(supported) || !isProtocolVersion(supported.min) ||
        !isProtocolVersion(supported.max) || supported.min > supported.max ||
        !hasString(input, "client_nonce") || supported.min > ATTACH_VERSION ||
        supported.max < ATTACH_VERSION) {
      throw new Error("invalid attach hello envelope");
    }
    return { ...input, kind: "negotiation", phase: "hello" };
  }

  if ("selected" in input || "server_nonce" in input) {
    rejectConflicts(input, [
      ...ENVELOPE_FIELDS, "protocol", "client", "supported", "client_nonce",
      "expires_at", "idle_timeout_seconds", "workspace_scopes",
    ]);
    if (input.selected !== ATTACH_VERSION || !hasString(input, "desktop_version") ||
        !hasString(input, "server_nonce") || !hasString(input, "authorization")) {
      throw new Error("invalid or incompatible attach welcome envelope");
    }
    return { ...input, kind: "negotiation", phase: "welcome" };
  }

  if ("workspace_scopes" in input) {
    rejectConflicts(input, [
      "protocol", "client", "supported", "client_nonce", "selected", "desktop_version",
      "server_nonce", "authorization", "approval_challenge", "request_id", "operation",
      "idempotency_key", "ok", "error", "subscription_id", "event", "run_id", "run_seq",
      "body",
    ]);
    if (!hasString(input, "capability") || !hasFiniteNumber(input, "expires_at") ||
        (input.expires_at as number) < 0 || !hasFiniteNumber(input, "idle_timeout_seconds") ||
        (input.idle_timeout_seconds as number) <= 0 || !isObject(input.workspace_scopes) ||
        !Object.values(input.workspace_scopes).every(
          (scopes) => Array.isArray(scopes) && scopes.every((scope) => typeof scope === "string"),
        )) {
      throw new Error("invalid attach authorization envelope");
    }
    return { ...input, kind: "authorization" };
  }

  requireProtocol(input);
  if (hasString(input, "operation")) {
    rejectConflicts(input, [
      ...NON_ENVELOPE_SHAPE_FIELDS, "ok", "error", "subscription_id", "event", "run_id", "run_seq",
    ]);
    if (!hasString(input, "request_id") || !hasString(input, "capability") || !isObject(input.body)) {
      throw new Error("invalid attach request envelope");
    }
    return { ...input, kind: "request" };
  }
  if (hasString(input, "event")) {
    rejectConflicts(input, [
      ...NON_ENVELOPE_SHAPE_FIELDS, "ok", "error", "request_id", "operation", "capability",
      "idempotency_key",
    ]);
    if (!hasString(input, "subscription_id") || !isObject(input.body)) {
      throw new Error("invalid attach event envelope");
    }
    return { ...input, kind: "event" };
  }
  if (typeof input.ok === "boolean") {
    rejectConflicts(input, [
      ...NON_ENVELOPE_SHAPE_FIELDS, "operation", "capability", "idempotency_key", "subscription_id",
      "event", "run_id", "run_seq",
    ]);
    if (!hasString(input, "request_id")) {
      throw new Error("invalid attach response envelope");
    }
    if (input.ok === true && !("error" in input) && isObject(input.body)) {
      return { ...input, kind: "response" };
    }
    if (input.ok === false && !("body" in input) && isObject(input.error) && hasString(input.error, "code") &&
        hasString(input.error, "message") && typeof input.error.retryable === "boolean") {
      return { ...input, kind: "error" };
    }
  }

  throw new Error("unrecognized attach envelope");
}

export function decodeAttachJson(bytes: string): AttachEnvelope {
  try {
    return decodeAttachEnvelope(JSON.parse(bytes) as unknown);
  } catch (error) {
    if (error instanceof SyntaxError) {
      throw new Error("attach envelope is not valid JSON", { cause: error });
    }
    throw error;
  }
}
