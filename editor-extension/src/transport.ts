import { randomBytes } from "node:crypto";
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
  dispose(): void;
}

export interface ConnectOptions {
  clientVersion: string;
  onPairingPending?: () => void;
  platform?: NodeJS.Platform;
  environment?: NodeJS.ProcessEnv;
  homedir?: () => string;
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

export function connectAttach(options: ConnectOptions): Promise<AttachConnection> {
  const platform = options.platform ?? process.platform;
  if (platform !== "linux" && platform !== "darwin") {
    return Promise.reject(new AttachTransportError("unsupported_platform"));
  }
  let runtime: string | undefined;
  try {
    runtime = platform === "linux"
      ? (options.environment ?? process.env).XDG_RUNTIME_DIR
      : posix.join((options.homedir ?? homedir)(), "Library", "Application Support", "Muniment", "runtime");
  } catch {
    return Promise.reject(new AttachTransportError("runtime_unavailable"));
  }
  if (!runtime || !posix.isAbsolute(runtime)) {
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
  const endpoint = platform === "linux"
    ? posix.join(runtime, "muniment", "attach-v1.sock")
    : posix.join(runtime, "attach-v1.sock");
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
          const onTerminal = (): void => {
            capability = "";
            socket.removeListener("error", onTerminal);
            socket.removeListener("close", onTerminal);
            socket.destroy();
          };
          socket.on("error", onTerminal);
          socket.on("close", onTerminal);
          resolve({
            get capability() { return capability; },
            expiresInSeconds: envelope.expires_at as number,
            idleTimeoutSeconds: envelope.idle_timeout_seconds as number,
            dispose(): void {
              capability = "";
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

function isHex(value: unknown, length: number): value is string {
  return typeof value === "string" && value.length === length && /^[0-9a-f]+$/i.test(value);
}

function validAuthorization(envelope: AttachEnvelope): boolean {
  const expires = envelope.expires_at;
  const idle = envelope.idle_timeout_seconds;
  return typeof expires === "number" && Number.isSafeInteger(expires) && expires > 0 && expires <= 8 * 60 * 60 &&
    typeof idle === "number" && Number.isSafeInteger(idle) && idle > 0 && idle <= 15 * 60;
}
