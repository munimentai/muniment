import { execFileSync } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { createConnection } from "node:net";
import { homedir } from "node:os";
import { posix } from "node:path";
import { TextDecoder } from "node:util";
import { ATTACH_PROTOCOL, decodeAttachJson, type AttachEnvelope } from "./protocol";

export const MAX_FRAME_LENGTH = 1024 * 1024;
const DEFAULT_IO_TIMEOUT_MS = 5_000;
const DEFAULT_APPROVAL_TIMEOUT_MS = 120_000;

export type AttachErrorCode =
  | "unsupported_platform"
  | "runtime_unavailable"
  | "desktop_unavailable"
  | "timeout"
  | "connection_closed"
  | "malformed_frame"
  | "payload_too_large"
  | "protocol_incompatible"
  | "randomness_unavailable"
  | "authorization_expired"
  | "request_rejected"
  | "desktop_failed"
  | "unexpected_message";

export class AttachTransportError extends Error {
  constructor(readonly code: AttachErrorCode) {
    super(code);
    this.name = "AttachTransportError";
  }
}

/** Incrementally decodes four-byte big-endian, UTF-8 JSON attach frames. */
export class AttachFrameDecoder {
  private prefix = Buffer.alloc(4);
  private prefixLength = 0;
  private payloadLength: number | undefined;
  private payloadParts: Buffer[] = [];
  private payloadBytes = 0;

  push(chunk: Uint8Array): AttachEnvelope[] {
    const frames: AttachEnvelope[] = [];
    let offset = 0;
    while (offset < chunk.length) {
      if (this.payloadLength === undefined) {
        const copied = Math.min(4 - this.prefixLength, chunk.length - offset);
        this.prefix.set(chunk.subarray(offset, offset + copied), this.prefixLength);
        this.prefixLength += copied;
        offset += copied;
        if (this.prefixLength < 4) continue;
        const declared = this.prefix.readUInt32BE(0);
        if (declared > MAX_FRAME_LENGTH) {
          this.reset();
          throw new AttachTransportError("payload_too_large");
        }
        this.payloadLength = declared;
      }

      const needed = this.payloadLength - this.payloadBytes;
      const copied = Math.min(needed, chunk.length - offset);
      if (copied > 0) {
        this.payloadParts.push(Buffer.from(chunk.subarray(offset, offset + copied)));
        this.payloadBytes += copied;
        offset += copied;
      }
      if (this.payloadBytes === this.payloadLength) {
        try {
          const payload = Buffer.concat(this.payloadParts, this.payloadLength);
          const json = new TextDecoder("utf-8", { fatal: true }).decode(payload);
          frames.push(decodeAttachJson(json));
        } catch (error) {
          this.reset();
          if (error instanceof AttachTransportError) throw error;
          if (error instanceof Error && error.message.includes("incompatible")) {
            throw new AttachTransportError("protocol_incompatible");
          }
          throw new AttachTransportError("malformed_frame");
        }
        this.reset();
      }
    }
    return frames;
  }

  reset(): void {
    this.prefixLength = 0;
    this.payloadLength = undefined;
    this.payloadParts = [];
    this.payloadBytes = 0;
  }
}

export function encodeAttachFrame(value: unknown): Buffer {
  const payload = Buffer.from(JSON.stringify(value), "utf8");
  if (payload.length > MAX_FRAME_LENGTH) throw new AttachTransportError("payload_too_large");
  const frame = Buffer.allocUnsafe(4 + payload.length);
  frame.writeUInt32BE(payload.length, 0);
  payload.copy(frame, 4);
  return frame;
}

interface AttachSocket {
  on(event: string, listener: (...args: any[]) => void): this;
  removeListener(event: string, listener: (...args: any[]) => void): this;
  write(bytes: Uint8Array): boolean;
  destroy(): this;
}

export interface AttachConnection {
  readonly capability: string;
  readonly expiresInSeconds: number;
  readonly idleTimeoutSeconds: number;
  listThreads(cursor?: string): Promise<ThreadListPage>;
  openThread(threadId: string, cursor?: string): Promise<ThreadOpenPage>;
  startRun(text: string, context?: JsonValue): Promise<RunStartAccepted>;
  streamRun(runId: string, afterRunSeq: number): Promise<RunStreamSubscription>;
  dispose(): void;
}

export interface Disposable { dispose(): void; }
export type Event<T> = (listener: (event: T) => void) => Disposable;

export interface RunStreamWindow { maxEvents: number; maxBytes: number; }
export interface ReceiptCapability { name: string; version: string; }
export interface RunReceipt {
  route?: string;
  model?: string;
  cost?: string;
  time?: string;
  capabilities: ReceiptCapability[];
}
export interface RedactedRunEvent {
  type: "run.event";
  runSeq: number;
  eventType: string;
  eventVersion: number;
  recordedAt: string;
  payload: { withheld: true; receipt?: RunReceipt };
}
export interface RunStreamCaughtUp {
  type: "subscription.caught_up";
  runSeq: number;
}
export interface RunStreamClosed {
  type: "stream.closed";
  runSeq: number;
  code: string;
  resumable: boolean;
}
export type RunStreamMessage = RedactedRunEvent | RunStreamCaughtUp | RunStreamClosed;
export interface RunStreamSubscription extends Disposable {
  readonly subscriptionId: string;
  readonly runId: string;
  readonly firstAvailableRunSeq: number;
  readonly currentRunSeq: number;
  readonly window: RunStreamWindow;
  readonly onDidReceiveMessage: Event<RunStreamMessage>;
  acknowledge(throughRunSeq: number): Promise<void>;
}

export type JsonValue = null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

export interface RunStartAccepted {
  runId: string;
  committedSeq: number;
  acceptedAt: string;
}

export interface RedactedThreadSummary {
  threadId: string;
  title: string;
  updatedAt: string;
}

export interface ThreadListPage {
  threads: RedactedThreadSummary[];
  nextCursor?: string;
}

export interface RedactedThreadEntry {
  runSeq: number;
  kind: string;
  text?: string;
}

export interface ThreadOpenPage {
  threadId: string;
  entries: RedactedThreadEntry[];
  nextCursor?: string;
}

export interface ConnectOptions {
  clientVersion: string;
  onPairingPending?: () => void;
  platform?: NodeJS.Platform;
  environment?: NodeJS.ProcessEnv;
  homedir?: () => string;
  discoverWindowsSid?: () => string;
  canonicalizeWindowsSid?: (sid: string) => Uint8Array;
  hashWindowsSid?: (sid: Uint8Array) => string;
  ioTimeoutMs?: number;
  approvalTimeoutMs?: number;
  createSocket?: (path: string) => AttachSocket;
  nonce?: () => string;
}

function transportError(error: unknown): AttachTransportError {
  return error instanceof AttachTransportError
    ? error
    : new AttachTransportError("desktop_unavailable");
}

function discoverWindowsSid(): string {
  const output = execFileSync("whoami.exe", ["/user", "/fo", "csv", "/nh"], {
    encoding: "utf8",
    windowsHide: true,
    timeout: DEFAULT_IO_TIMEOUT_MS,
    maxBuffer: 64 * 1024,
  });
  const fields = output.match(/"(?:[^"]|"")*"/g) ?? [];
  const sid = fields[1]?.slice(1, -1).replace(/""/g, '"');
  if (fields.length !== 2 || !sid || !/^S-\d+(?:-\d+)+$/i.test(sid)) {
    throw new Error("SID unavailable");
  }
  return sid;
}

/** Converts an SDDL SID string to the binary SID representation hashed by ADR 0009. */
function canonicalizeWindowsSid(value: string): Uint8Array {
  const parts = value.split("-");
  if (parts.length < 4 || parts[0].toUpperCase() !== "S") throw new Error("invalid SID");
  const revision = parseDecimal(parts[1], 0xffn);
  const authority = parseDecimal(parts[2], 0xffffffffffffn);
  const subAuthorities = parts.slice(3).map((part) => parseDecimal(part, 0xffffffffn));
  if (revision !== 1n || subAuthorities.length > 15) throw new Error("invalid SID");

  const bytes = Buffer.alloc(8 + subAuthorities.length * 4);
  bytes[0] = Number(revision);
  bytes[1] = subAuthorities.length;
  let remainingAuthority = authority;
  for (let index = 7; index >= 2; index--) {
    bytes[index] = Number(remainingAuthority & 0xffn);
    remainingAuthority >>= 8n;
  }
  subAuthorities.forEach((part, index) => bytes.writeUInt32LE(Number(part), 8 + index * 4));
  return bytes;
}

function parseDecimal(value: string | undefined, maximum: bigint): bigint {
  if (!value || !/^(0|[1-9]\d*)$/.test(value)) throw new Error("invalid SID");
  const parsed = BigInt(value);
  if (parsed > maximum) throw new Error("invalid SID");
  return parsed;
}

function hashWindowsSid(sid: Uint8Array): string {
  return createHash("sha256").update(sid).digest("hex").slice(0, 32);
}

export function connectAttach(options: ConnectOptions): Promise<AttachConnection> {
  const platform = options.platform ?? process.platform;
  if (platform !== "linux" && platform !== "darwin" && platform !== "win32") {
    return Promise.reject(new AttachTransportError("unsupported_platform"));
  }
  let endpoint: string;
  try {
    if (platform === "win32") {
      const sid = (options.discoverWindowsSid ?? discoverWindowsSid)();
      const canonical = (options.canonicalizeWindowsSid ?? canonicalizeWindowsSid)(sid);
      const hash = (options.hashWindowsSid ?? hashWindowsSid)(canonical);
      if (!/^[0-9a-f]{32}$/.test(hash)) throw new Error("invalid SID hash");
      endpoint = `\\\\.\\pipe\\Muniment\\attach-v1-${hash}`;
    } else {
      const runtime = platform === "linux"
        ? (options.environment ?? process.env).XDG_RUNTIME_DIR
        : posix.join((options.homedir ?? homedir)(), "Library", "Application Support", "Muniment", "runtime");
      if (!runtime || !posix.isAbsolute(runtime)) throw new Error("invalid runtime");
      endpoint = platform === "linux"
        ? posix.join(runtime, "muniment", "attach-v1.sock")
        : posix.join(runtime, "attach-v1.sock");
    }
  } catch {
    return Promise.reject(new AttachTransportError("runtime_unavailable"));
  }

  const ioTimeout = options.ioTimeoutMs ?? DEFAULT_IO_TIMEOUT_MS;
  const approvalTimeout = options.approvalTimeoutMs ?? DEFAULT_APPROVAL_TIMEOUT_MS;
  if (!Number.isFinite(ioTimeout) || ioTimeout <= 0 ||
      !Number.isFinite(approvalTimeout) || approvalTimeout <= 0) {
    return Promise.reject(new AttachTransportError("unexpected_message"));
  }
  let nonce: string;
  try {
    nonce = options.nonce?.() ?? randomBytes(16).toString("hex");
  } catch {
    return Promise.reject(new AttachTransportError("randomness_unavailable"));
  }
  if (!isHex(nonce, 32)) {
    return Promise.reject(new AttachTransportError("randomness_unavailable"));
  }
  let socket: AttachSocket;
  try {
    socket = (options.createSocket ?? ((path) => createConnection({ path })))(endpoint);
  } catch {
    return Promise.reject(new AttachTransportError("desktop_unavailable"));
  }
  const decoder = new AttachFrameDecoder();

  return new Promise((resolve, reject) => {
    let phase: "connecting" | "welcome" | "authorization" | "done" = "connecting";
    let timer: NodeJS.Timeout | undefined;
    let settled = false;

    const clearTimer = (): void => {
      if (timer) clearTimeout(timer);
      timer = undefined;
    };
    const armTimer = (milliseconds: number): void => {
      clearTimer();
      timer = setTimeout(() => fail(new AttachTransportError("timeout")), milliseconds);
    };
    const removeHandshakeListeners = (): void => {
      socket.removeListener("connect", onConnect);
      socket.removeListener("data", onData);
      socket.removeListener("error", onError);
      socket.removeListener("close", onClose);
    };
    const fail = (error: unknown): void => {
      if (settled) return;
      settled = true;
      clearTimer();
      decoder.reset();
      removeHandshakeListeners();
      socket.destroy();
      reject(transportError(error));
    };
    const onConnect = (): void => {
      phase = "welcome";
      try {
        socket.write(encodeAttachFrame({
          protocol: ATTACH_PROTOCOL,
          client: { kind: "editor-extension", version: options.clientVersion },
          supported: { min: 1, max: 1 },
          client_nonce: nonce,
        }));
        armTimer(ioTimeout);
      } catch (error) {
        fail(error);
      }
    };
    const onData = (chunk: Buffer): void => {
      try {
        for (const envelope of decoder.push(chunk)) {
          if (envelope.kind === "error") {
            const error = envelope.error as Record<string, unknown>;
            fail(new AttachTransportError(error.code === "protocol_incompatible"
              ? "protocol_incompatible" : "unexpected_message"));
            return;
          }
          if (phase === "welcome") {
            if (envelope.kind !== "negotiation" || envelope.phase !== "welcome" ||
                envelope.authorization !== "pairing_required" ||
                !isHex(envelope.server_nonce, 32) || !isHex(envelope.approval_challenge, 32)) {
              fail(new AttachTransportError("unexpected_message"));
              return;
            }
            phase = "authorization";
            options.onPairingPending?.();
            armTimer(approvalTimeout);
            continue;
          }
          if (phase !== "authorization" || envelope.kind !== "authorization" ||
              !isHex(envelope.capability, 64) || !validAuthorization(envelope)) {
            fail(new AttachTransportError("unexpected_message"));
            return;
          }
          settled = true;
          phase = "done";
          clearTimer();
          removeHandshakeListeners();
          let capability = envelope.capability as string;
          const pending = new Map<string, {
            requestId: string;
            resolve: (envelope: AttachEnvelope) => void;
            reject: (error: AttachTransportError) => void;
            timer: NodeJS.Timeout;
            owner?: Set<string>;
            onResponse?: (envelope: AttachEnvelope) => void;
          }>();
          const subscriptions = new Map<string, ActiveRunSubscription>();
          const ignoredResponses = new Set<string>();
          const rejectPending = (error: AttachTransportError): void => {
            for (const item of pending.values()) {
              clearTimeout(item.timer);
              item.reject(error);
            }
            pending.clear();
          };
          const onTerminal = (): void => {
            capability = "";
            rejectPending(new AttachTransportError("connection_closed"));
            for (const subscription of subscriptions.values()) subscription.dispose();
            subscriptions.clear();
            socket.removeListener("data", onAuthorizedData);
            socket.removeListener("error", onTerminal);
            socket.removeListener("close", onTerminal);
            socket.destroy();
          };
          const closeUnexpected = (error: AttachTransportError): void => {
            rejectPending(error);
            onTerminal();
          };
          const onAuthorizedData = (chunk: Buffer): void => {
            try {
              for (const message of decoder.push(chunk)) {
                if (message.kind === "event") {
                  const subscription = subscriptions.get(message.subscription_id as string);
                  if (!subscription) {
                    closeUnexpected(new AttachTransportError("unexpected_message"));
                    return;
                  }
                  subscription.deliver(message);
                  continue;
                }
                if (message.kind !== "response" && message.kind !== "error") {
                  closeUnexpected(new AttachTransportError("unexpected_message"));
                  return;
                }
                const responseId = message.request_id as string;
                if (ignoredResponses.delete(responseId)) continue;
                if (!pending.has(responseId)) {
                  closeUnexpected(new AttachTransportError("unexpected_message"));
                  return;
                }
                const current = pending.get(message.request_id as string)!;
                clearTimeout(current.timer);
                pending.delete(current.requestId);
                current.owner?.delete(current.requestId);
                if (message.kind === "error") {
                  const code = (message.error as Record<string, unknown>).code;
                  current.reject(new AttachTransportError(code === "protocol_incompatible"
                    ? "protocol_incompatible"
                    : code === "unauthorized" ? "authorization_expired"
                    : code === "persistence_failed" ? "desktop_failed"
                    : "request_rejected"));
                } else {
                  try {
                    current.onResponse?.(message);
                  } catch (error) {
                    current.reject(transportError(error));
                    throw error;
                  }
                  current.resolve(message);
                }
              }
            } catch (error) {
              closeUnexpected(transportError(error));
            }
          };
          const request = (operation: "thread.list" | "thread.open" | "run.start" | "run.stream" |
            "run.cursor_ack", body: JsonBody,
            idempotent = false, owner?: Set<string>,
            onResponse?: (envelope: AttachEnvelope) => void): Promise<AttachEnvelope> => {
            if (!capability) return Promise.reject(new AttachTransportError("authorization_expired"));
            let requestId: string;
            let idempotencyKey: string | undefined;
            try {
              requestId = freshRequestId();
              if (idempotent) idempotencyKey = freshRequestId();
            } catch {
              return Promise.reject(new AttachTransportError("randomness_unavailable"));
            }
            return new Promise((requestResolve, requestReject) => {
              const timer = setTimeout(() => {
                closeUnexpected(new AttachTransportError("timeout"));
              }, ioTimeout);
              pending.set(requestId, { requestId, resolve: requestResolve, reject: requestReject,
                timer, owner, onResponse });
              owner?.add(requestId);
              try {
                socket.write(encodeAttachFrame({
                  protocol: ATTACH_PROTOCOL, request_id: requestId, operation, capability, body,
                  ...(idempotencyKey === undefined ? {} : { idempotency_key: idempotencyKey }),
                }));
              } catch (error) {
                closeUnexpected(transportError(error));
              }
            });
          };
          socket.on("data", onAuthorizedData);
          socket.on("error", onTerminal);
          socket.on("close", onTerminal);
          resolve({
            get capability() { return capability; },
            expiresInSeconds: envelope.expires_at as number,
            idleTimeoutSeconds: envelope.idle_timeout_seconds as number,
            async listThreads(cursor?: string): Promise<ThreadListPage> {
              validateCursor(cursor, MAX_TEXT_LENGTH);
              const body: JsonBody = { limit: THREAD_PAGE_LIMIT };
              if (cursor !== undefined) body.cursor = cursor;
              return decodeThreadListPage((await request("thread.list", body)).body);
            },
            async openThread(threadId: string, cursor?: string): Promise<ThreadOpenPage> {
              validateBoundedString(threadId, MAX_THREAD_ID_LENGTH, false);
              validateCursor(cursor, MAX_CURSOR_LENGTH);
              const body: JsonBody = { thread_id: threadId, limit: THREAD_PAGE_LIMIT };
              if (cursor !== undefined) body.cursor = cursor;
              return decodeThreadOpenPage((await request("thread.open", body)).body, threadId);
            },
            async startRun(text: string, context?: JsonValue): Promise<RunStartAccepted> {
              const validatedContext = validateRunStartInput(text, context);
              const body: JsonBody = { text };
              if (validatedContext !== undefined) body.context = validatedContext;
              return decodeRunStartAccepted((await request("run.start", body, true)).body);
            },
            async streamRun(runId: string, afterRunSeq: number): Promise<RunStreamSubscription> {
              validateRunStreamInput(runId, afterRunSeq);
              let state: ActiveRunSubscription | undefined;
              await request("run.stream", { run_id: runId, after_run_seq: afterRunSeq }, false,
                undefined, (response) => {
                  const summary = decodeRunStreamSummary(response.body, runId, afterRunSeq);
                  if (subscriptions.has(summary.subscriptionId)) {
                    throw new AttachTransportError("unexpected_message");
                  }
                  state = createRunSubscription(summary, afterRunSeq, request, pending,
                    ignoredResponses, () => subscriptions.delete(summary.subscriptionId));
                  subscriptions.set(summary.subscriptionId, state);
                });
              if (!state) throw new AttachTransportError("unexpected_message");
              return state.public;
            },
            dispose(): void {
              capability = "";
              rejectPending(new AttachTransportError("connection_closed"));
              for (const subscription of subscriptions.values()) subscription.dispose();
              subscriptions.clear();
              socket.removeListener("data", onAuthorizedData);
              socket.removeListener("error", onTerminal);
              socket.removeListener("close", onTerminal);
              socket.destroy();
            },
          });
          return;
        }
      } catch (error) {
        fail(error);
      }
    };
    const onError = (): void => fail(new AttachTransportError("desktop_unavailable"));
    const onClose = (): void => fail(new AttachTransportError("connection_closed"));

    socket.on("connect", onConnect);
    socket.on("data", onData);
    socket.on("error", onError);
    socket.on("close", onClose);
    armTimer(ioTimeout);
  });
}

type JsonBody = Record<string, JsonValue>;
const THREAD_PAGE_LIMIT = 100;
const MAX_TEXT_LENGTH = 64 * 1024;
const MAX_RUN_START_TEXT_LENGTH = 32 * 1024;
const MAX_RUN_START_CONTEXT_LENGTH = 64 * 1024;
const MAX_CURSOR_LENGTH = 1024;
const MAX_THREAD_ID_LENGTH = 36;
const MAX_SUBSCRIPTION_ID_LENGTH = 64;
const MAX_RUN_STREAM_WINDOW_EVENTS = 1024;
const MAX_RUN_STREAM_WINDOW_BYTES = 4 * 1024 * 1024;

interface RunStreamSummary {
  subscriptionId: string;
  runId: string;
  firstAvailableRunSeq: number;
  currentRunSeq: number;
  window: RunStreamWindow;
}

interface ActiveRunSubscription {
  public: RunStreamSubscription;
  deliver(envelope: AttachEnvelope): void;
  dispose(): void;
}

function validateRunStreamInput(runId: unknown, afterRunSeq: unknown): void {
  if (!isUuid(runId) || typeof afterRunSeq !== "number" ||
      !Number.isSafeInteger(afterRunSeq) || afterRunSeq < 0) {
    throw new AttachTransportError("unexpected_message");
  }
}

function decodeRunStreamSummary(value: unknown, expectedRunId: string,
  afterRunSeq: number): RunStreamSummary {
  const summary = exactObject(value, ["subscription_id", "run_id", "first_available_run_seq",
    "current_run_seq", "window"]);
  const window = exactObject(summary.window, ["max_events", "max_bytes"]);
  validateBoundedString(summary.subscription_id, MAX_SUBSCRIPTION_ID_LENGTH, false);
  const first = summary.first_available_run_seq;
  const current = summary.current_run_seq;
  if (summary.run_id !== expectedRunId || typeof first !== "number" ||
      !Number.isSafeInteger(first) || first <= 0 || typeof current !== "number" ||
      !Number.isSafeInteger(current) || current < first || afterRunSeq > current ||
      afterRunSeq + 1 < first || !boundedPositiveInteger(window.max_events,
        MAX_RUN_STREAM_WINDOW_EVENTS) || !boundedPositiveInteger(window.max_bytes,
        MAX_RUN_STREAM_WINDOW_BYTES)) {
    throw new AttachTransportError("unexpected_message");
  }
  return { subscriptionId: summary.subscription_id, runId: expectedRunId,
    firstAvailableRunSeq: first, currentRunSeq: current,
    window: { maxEvents: window.max_events, maxBytes: window.max_bytes } };
}

function createRunSubscription(summary: RunStreamSummary, afterRunSeq: number,
  request: (operation: "run.cursor_ack", body: JsonBody, idempotent?: boolean,
    owner?: Set<string>) => Promise<AttachEnvelope>,
  pending: Map<string, { reject: (error: AttachTransportError) => void; timer: NodeJS.Timeout;
    owner?: Set<string> }>, ignoredResponses: Set<string>,
  onDispose: () => void): ActiveRunSubscription {
  let disposed = false;
  let highest = afterRunSeq;
  let acknowledged = afterRunSeq;
  let acknowledging = false;
  let caughtUp = false;
  const listeners = new Set<(event: RunStreamMessage) => void>();
  const queuedMessages: RunStreamMessage[] = [];
  const acknowledgementRequests = new Set<string>();
  const dispose = (): void => {
    if (disposed) return;
    disposed = true;
    listeners.clear();
    queuedMessages.length = 0;
    for (const requestId of acknowledgementRequests) {
      const item = pending.get(requestId);
      if (!item) continue;
      clearTimeout(item.timer);
      pending.delete(requestId);
      ignoredResponses.add(requestId);
      item.reject(new AttachTransportError("connection_closed"));
    }
    acknowledgementRequests.clear();
    onDispose();
  };
  const closeWithMessage = (): void => {
    if (disposed) return;
    disposed = true;
    listeners.clear();
    for (const requestId of acknowledgementRequests) {
      const item = pending.get(requestId);
      if (!item) continue;
      clearTimeout(item.timer);
      pending.delete(requestId);
      ignoredResponses.add(requestId);
      item.reject(new AttachTransportError("connection_closed"));
    }
    acknowledgementRequests.clear();
    onDispose();
  };
  const state: ActiveRunSubscription = {
    dispose,
    deliver(envelope): void {
      if (disposed || envelope.subscription_id !== summary.subscriptionId ||
          envelope.run_id !== summary.runId) throw new AttachTransportError("unexpected_message");
      const runSeq = envelope.run_seq;
      if (typeof runSeq !== "number" || !Number.isSafeInteger(runSeq) || runSeq <= 0) {
        throw new AttachTransportError("unexpected_message");
      }
      let message: RunStreamMessage;
      if (envelope.event === "run.event") {
        if (highest + 1 !== runSeq || (!caughtUp && runSeq > summary.currentRunSeq) ||
            runSeq - acknowledged > summary.window.maxEvents) {
          throw new AttachTransportError("unexpected_message");
        }
        const body = exactObject(envelope.body, ["event_type", "event_version", "payload", "recorded_at"]);
        const payload = exactObject(body.payload, ["withheld"], ["receipt"]);
        validateBoundedString(body.event_type, MAX_TEXT_LENGTH, false);
        if ((body.event_type as string).trim().length === 0 ||
            typeof body.event_version !== "number" || !Number.isSafeInteger(body.event_version) ||
            body.event_version <= 0 || typeof body.recorded_at !== "string" ||
            !isRfc3339(body.recorded_at) || payload.withheld !== true ||
            (payload.receipt !== undefined && body.event_type !== "run.completed")) {
          throw new AttachTransportError("unexpected_message");
        }
        const receipt = payload.receipt === undefined ? undefined : decodeRunReceipt(payload.receipt);
        highest = runSeq;
        message = { type: "run.event", runSeq, eventType: body.event_type,
          eventVersion: body.event_version, recordedAt: body.recorded_at,
          payload: { withheld: true, ...(receipt === undefined ? {} : { receipt }) } };
      } else if (envelope.event === "subscription.caught_up") {
        if (caughtUp || runSeq !== summary.currentRunSeq || highest !== summary.currentRunSeq ||
            Object.keys(exactObject(envelope.body, [])).length !== 0) {
          throw new AttachTransportError("unexpected_message");
        }
        caughtUp = true;
        message = { type: "subscription.caught_up", runSeq };
      } else if (envelope.event === "stream.closed") {
        if (runSeq !== highest + 1) throw new AttachTransportError("unexpected_message");
        const body = exactObject(envelope.body, ["code", "resumable"]);
        validateBoundedString(body.code, MAX_TEXT_LENGTH, false);
        if (typeof body.resumable !== "boolean") throw new AttachTransportError("unexpected_message");
        highest = runSeq;
        message = { type: "stream.closed", runSeq, code: body.code, resumable: body.resumable };
      } else {
        throw new AttachTransportError("unexpected_message");
      }
      if (listeners.size === 0) queuedMessages.push(message);
      else for (const listener of [...listeners]) {
        try { listener(message); } catch { /* Consumer errors do not invalidate transport. */ }
      }
      if (message.type === "stream.closed") closeWithMessage();
    },
    public: {
      subscriptionId: summary.subscriptionId,
      runId: summary.runId,
      firstAvailableRunSeq: summary.firstAvailableRunSeq,
      currentRunSeq: summary.currentRunSeq,
      window: summary.window,
      onDidReceiveMessage(listener): Disposable {
        if (disposed && queuedMessages.length === 0) return { dispose() {} };
        listeners.add(listener);
        for (const message of queuedMessages.splice(0)) {
          try { listener(message); } catch { /* Match VS Code event listener isolation. */ }
        }
        if (disposed) listeners.delete(listener);
        return { dispose: () => listeners.delete(listener) };
      },
      async acknowledge(throughRunSeq: number): Promise<void> {
        if (disposed || acknowledging || typeof throughRunSeq !== "number" ||
            !Number.isSafeInteger(throughRunSeq) ||
            throughRunSeq <= acknowledged || throughRunSeq > highest) {
          throw new AttachTransportError("unexpected_message");
        }
        acknowledging = true;
        let response: AttachEnvelope;
        try {
          response = await request("run.cursor_ack", {
            subscription_id: summary.subscriptionId, through_run_seq: throughRunSeq,
          }, false, acknowledgementRequests);
        } finally {
          acknowledging = false;
        }
        if (disposed) throw new AttachTransportError("connection_closed");
        const body = exactObject(response.body, ["subscription_id", "through_run_seq"]);
        if (body.subscription_id !== summary.subscriptionId || body.through_run_seq !== throughRunSeq) {
          throw new AttachTransportError("unexpected_message");
        }
        acknowledged = throughRunSeq;
      },
      dispose,
    },
  };
  return state;
}

function boundedPositiveInteger(value: unknown, maximum: number): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0 && value <= maximum;
}

function decodeRunReceipt(value: unknown): RunReceipt {
  const receipt = exactObject(value, ["capabilities"], ["route", "model", "cost", "time"]);
  for (const field of ["route", "model", "cost", "time"] as const) {
    if (receipt[field] !== undefined && receipt[field] !== null) {
      validateBoundedString(receipt[field], 1_024, false);
      if ((receipt[field] as string).trim().length === 0) {
        throw new AttachTransportError("unexpected_message");
      }
    }
  }
  if (!Array.isArray(receipt.capabilities) || receipt.capabilities.length > 64) {
    throw new AttachTransportError("unexpected_message");
  }
  const capabilities = receipt.capabilities.map((value): ReceiptCapability => {
    const capability = exactObject(value, ["name", "version"]);
    validateBoundedString(capability.name, 1_024, false);
    validateBoundedString(capability.version, 1_024, false);
    if (capability.name.trim().length === 0 || capability.version.trim().length === 0) {
      throw new AttachTransportError("unexpected_message");
    }
    return { name: capability.name, version: capability.version };
  });
  return {
    ...(typeof receipt.route !== "string" ? {} : { route: receipt.route }),
    ...(typeof receipt.model !== "string" ? {} : { model: receipt.model }),
    ...(typeof receipt.cost !== "string" ? {} : { cost: receipt.cost }),
    ...(typeof receipt.time !== "string" ? {} : { time: receipt.time }),
    capabilities,
  };
}

function freshRequestId(): string {
  const bytes = randomBytes(16);
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = bytes.toString("hex");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

function validateBoundedString(value: unknown, maximum: number, allowEmpty: boolean): asserts value is string {
  if (typeof value !== "string" || (!allowEmpty && value.length === 0) || Buffer.byteLength(value) > maximum) {
    throw new AttachTransportError("unexpected_message");
  }
}

function validateCursor(cursor: string | undefined, maximum: number): void {
  if (cursor !== undefined) validateBoundedString(cursor, maximum, false);
}

function validateRunStartInput(text: unknown, context: unknown): JsonValue | undefined {
  if (typeof text !== "string" || text.trim().length === 0 ||
      Buffer.byteLength(text) > MAX_RUN_START_TEXT_LENGTH) {
    throw new AttachTransportError("unexpected_message");
  }
  if (context !== undefined) {
    try {
      if (!isJsonValue(context)) throw new AttachTransportError("unexpected_message");
      const encoded = JSON.stringify(context);
      if (encoded === undefined || Buffer.byteLength(encoded) > MAX_RUN_START_CONTEXT_LENGTH) {
        throw new AttachTransportError("unexpected_message");
      }
      const snapshot = JSON.parse(encoded) as unknown;
      if (!isJsonValue(snapshot)) throw new AttachTransportError("unexpected_message");
      return snapshot;
    } catch {
      throw new AttachTransportError("unexpected_message");
    }
  }
  return undefined;
}

function exactObject(value: unknown, required: readonly string[], optional: readonly string[] = []): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new AttachTransportError("unexpected_message");
  }
  const object = value as Record<string, unknown>;
  const allowed = new Set([...required, ...optional]);
  if (!required.every((key) => key in object) || Object.keys(object).some((key) => !allowed.has(key))) {
    throw new AttachTransportError("unexpected_message");
  }
  return object;
}

function decodeThreadListPage(value: unknown): ThreadListPage {
  const page = exactObject(value, ["threads"], ["next_cursor"]);
  if (!Array.isArray(page.threads) || page.threads.length > THREAD_PAGE_LIMIT) {
    throw new AttachTransportError("unexpected_message");
  }
  const threads = page.threads.map((value): RedactedThreadSummary => {
    const thread = exactObject(value, ["thread_id", "title", "updated_at"]);
    validateBoundedString(thread.thread_id, MAX_TEXT_LENGTH, false);
    validateBoundedString(thread.title, MAX_TEXT_LENGTH, true);
    validateBoundedString(thread.updated_at, MAX_TEXT_LENGTH, false);
    return { threadId: thread.thread_id, title: thread.title, updatedAt: thread.updated_at };
  });
  validateOptionalCursor(page.next_cursor, MAX_TEXT_LENGTH);
  return { threads, ...(page.next_cursor === undefined ? {} : { nextCursor: page.next_cursor }) };
}

function decodeThreadOpenPage(value: unknown, expectedThreadId: string): ThreadOpenPage {
  const page = exactObject(value, ["thread_id", "entries"], ["next_cursor"]);
  if (page.thread_id !== expectedThreadId || !Array.isArray(page.entries) ||
      page.entries.length > THREAD_PAGE_LIMIT) {
    throw new AttachTransportError("unexpected_message");
  }
  const entries = page.entries.map((value): RedactedThreadEntry => {
    const entry = exactObject(value, ["run_seq", "kind"], ["text"]);
    if (typeof entry.run_seq !== "number" || !Number.isSafeInteger(entry.run_seq) || entry.run_seq <= 0) {
      throw new AttachTransportError("unexpected_message");
    }
    validateBoundedString(entry.kind, MAX_TEXT_LENGTH, false);
    if (entry.text !== undefined) validateBoundedString(entry.text, MAX_TEXT_LENGTH, true);
    return { runSeq: entry.run_seq, kind: entry.kind, ...(entry.text === undefined ? {} : { text: entry.text }) };
  });
  validateOptionalCursor(page.next_cursor, MAX_CURSOR_LENGTH);
  return { threadId: expectedThreadId, entries,
    ...(page.next_cursor === undefined ? {} : { nextCursor: page.next_cursor }) };
}

function decodeRunStartAccepted(value: unknown): RunStartAccepted {
  const accepted = exactObject(value, ["run_id", "committed_seq", "accepted_at"]);
  if (!isUuid(accepted.run_id) || typeof accepted.committed_seq !== "number" ||
      !Number.isSafeInteger(accepted.committed_seq) || accepted.committed_seq <= 0 ||
      typeof accepted.accepted_at !== "string" || !isRfc3339(accepted.accepted_at)) {
    throw new AttachTransportError("unexpected_message");
  }
  return { runId: accepted.run_id, committedSeq: accepted.committed_seq,
    acceptedAt: accepted.accepted_at };
}

function validateOptionalCursor(value: unknown, maximum: number): asserts value is string | undefined {
  if (value !== undefined) validateBoundedString(value, maximum, false);
}

function isHex(value: unknown, length: number): value is string {
  return typeof value === "string" && value.length === length && /^[0-9a-f]+$/i.test(value);
}

function isUuid(value: unknown): value is string {
  return typeof value === "string" && (/^[0-9a-f]{32}$/i.test(value) ||
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value));
}

function isJsonValue(value: unknown, seen = new Set<object>()): value is JsonValue {
  if (value === null || typeof value === "string" || typeof value === "boolean") return true;
  if (typeof value === "number") return Number.isFinite(value);
  if (typeof value !== "object" || seen.has(value)) return false;
  seen.add(value);
  const valid = Array.isArray(value)
    ? value.every((item) => isJsonValue(item, seen))
    : Object.getPrototypeOf(value) === Object.prototype &&
      Object.values(value).every((item) => isJsonValue(item, seen));
  seen.delete(value);
  return valid;
}

function isRfc3339(value: string): boolean {
  if (Buffer.byteLength(value) > MAX_TEXT_LENGTH) return false;
  const match = /^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:[Zz]|([+-])(\d{2}):(\d{2}))$/.exec(value);
  if (!match) return false;
  const [, year, month, day, hour, minute, second, , zoneHour, zoneMinute] = match;
  const y = Number(year), m = Number(month), d = Number(day);
  const leap = y % 4 === 0 && (y % 100 !== 0 || y % 400 === 0);
  const days = [0, 31, leap ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  return m >= 1 && m <= 12 && d >= 1 && d <= days[m] && Number(hour) <= 23 &&
    Number(minute) <= 59 && Number(second) <= 60 &&
    (zoneHour === undefined || (Number(zoneHour) <= 23 && Number(zoneMinute) <= 59));
}

function validAuthorization(envelope: AttachEnvelope): boolean {
  const expires = envelope.expires_at;
  const idle = envelope.idle_timeout_seconds;
  return typeof expires === "number" && Number.isSafeInteger(expires) && expires > 0 && expires <= 8 * 60 * 60 &&
    typeof idle === "number" && Number.isSafeInteger(idle) && idle > 0 && idle <= 15 * 60;
}
