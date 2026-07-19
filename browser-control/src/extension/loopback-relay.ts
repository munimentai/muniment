import { RelayConnection, type RelayTransport } from '../index.js';
import type { AuthorizedRelayProvider } from './relay-provider.js';

export const PAIRING_PROTOCOL = 'muniment-pairing';
export const HEARTBEAT_INTERVAL_MS = 20_000;
export const HEARTBEAT_ACK_TIMEOUT_MS = 9_000;

export interface RelayPairingHandoff {
  endpoint: string;
  token: string;
}

export interface RelayWebSocket {
  readonly readyState: number;
  readonly protocol: string;
  binaryType: BinaryType;
  send(data: string): void;
  close(code?: number, reason?: string): void;
  addEventListener(type: 'open' | 'error' | 'close', listener: (event: Event) => void): void;
  addEventListener(type: 'message', listener: (event: MessageEvent<unknown>) => void): void;
  removeEventListener(type: 'open' | 'error' | 'close', listener: (event: Event) => void): void;
  removeEventListener(type: 'message', listener: (event: MessageEvent<unknown>) => void): void;
}

export type RelayWebSocketFactory = (endpoint: string, protocols: string[]) => RelayWebSocket;

interface TimerApi {
  setInterval(callback: () => void, delay: number): ReturnType<typeof setInterval>;
  clearInterval(timer: ReturnType<typeof setInterval>): void;
  setTimeout(callback: () => void, delay: number): ReturnType<typeof setTimeout>;
  clearTimeout(timer: ReturnType<typeof setTimeout>): void;
}

const nativeTimers: TimerApi = {
  setInterval: (callback, delay) => setInterval(callback, delay),
  clearInterval: timer => clearInterval(timer),
  setTimeout: (callback, delay) => setTimeout(callback, delay),
  clearTimeout: timer => clearTimeout(timer),
};

/** One-shot, in-memory bridge from a desktop pairing handoff to an authorized relay. */
export class LoopbackRelayAdapter {
  #attempt: SocketAttempt | undefined;

  constructor(
    private readonly provider: AuthorizedRelayProvider,
    private readonly createSocket: RelayWebSocketFactory = (endpoint, protocols) => new WebSocket(endpoint, protocols),
    private readonly timers: TimerApi = nativeTimers,
  ) {}

  accept(handoff: RelayPairingHandoff): boolean {
    this.close();
    if (!isPairingHandoff(handoff) || !isNumericLoopbackWebSocket(handoff.endpoint))
      return false;

    let token: string | undefined = handoff.token;
    let socket: RelayWebSocket;
    try {
      socket = this.createSocket(handoff.endpoint, [PAIRING_PROTOCOL, token]);
    } catch {
      token = undefined;
      return false;
    }
    token = undefined;

    const attempt = new SocketAttempt(socket, this.provider, this.timers, () => {
      if (this.#attempt === attempt)
        this.#attempt = undefined;
    });
    this.#attempt = attempt;
    return true;
  }

  close(): void {
    const attempt = this.#attempt;
    this.#attempt = undefined;
    attempt?.close(1000, 'Relay revoked');
  }
}

class SocketAttempt implements RelayTransport {
  readonly #messageListeners = new Set<(message: string | Uint8Array) => void>();
  readonly #closeListeners = new Set<() => void>();
  #heartbeat: ReturnType<typeof setInterval> | undefined;
  #heartbeatDeadline: ReturnType<typeof setTimeout> | undefined;
  #relay: RelayConnection | undefined;
  #terminal = false;
  #opened = false;

  constructor(
    private readonly socket: RelayWebSocket,
    private readonly provider: AuthorizedRelayProvider,
    private readonly timers: TimerApi,
    private readonly onTerminal: () => void,
  ) {
    socket.binaryType = 'arraybuffer';
    socket.addEventListener('open', this.#onOpen);
    socket.addEventListener('message', this.#onMessage);
    socket.addEventListener('error', this.#onError);
    socket.addEventListener('close', this.#onRemoteClose);
  }

  send(message: string): void {
    if (this.#terminal || !this.#opened)
      throw new Error('Relay transport is not open');
    this.socket.send(message);
  }

  close(code = 1000, reason = ''): void {
    if (this.#terminal)
      return;
    this.#terminate();
    try { this.socket.close(code, reason); } catch { /* already terminal */ }
  }

  onMessage(listener: (message: string | Uint8Array) => void): () => void {
    if (!this.#terminal)
      this.#messageListeners.add(listener);
    return () => this.#messageListeners.delete(listener);
  }

  onClose(listener: () => void): () => void {
    if (this.#terminal) {
      listener();
      return () => {};
    }
    this.#closeListeners.add(listener);
    return () => this.#closeListeners.delete(listener);
  }

  readonly #onOpen = (): void => {
    if (this.#terminal || this.#opened)
      return;
    if (this.socket.protocol !== PAIRING_PROTOCOL) {
      this.close(1002, 'Pairing protocol rejected');
      return;
    }
    this.#opened = true;
    this.#heartbeat = this.timers.setInterval(() => {
      const relay = this.#relay;
      if (!relay || this.#heartbeatDeadline !== undefined)
        return;
      this.#heartbeatDeadline = this.timers.setTimeout(
        () => this.close(1011, 'Heartbeat acknowledgement timed out'),
        HEARTBEAT_ACK_TIMEOUT_MS,
      );
      void relay.send('muniment.heartbeat').then(
        () => this.#clearHeartbeatDeadline(),
        () => this.close(1011, 'Heartbeat failed'),
      );
    }, HEARTBEAT_INTERVAL_MS);
    try {
      const relay = new RelayConnection(this);
      this.#relay = relay;
      this.provider.accept(relay);
    } catch {
      this.close(1011, 'Relay composition failed');
    }
  };

  readonly #onMessage = (event: MessageEvent<unknown>): void => {
    if (this.#terminal)
      return;
    const data = event.data;
    let message: string | Uint8Array;
    if (typeof data === 'string')
      message = data;
    else if (data instanceof ArrayBuffer)
      message = new Uint8Array(data);
    else if (ArrayBuffer.isView(data))
      message = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    else {
      this.close(1003, 'Unsupported relay message');
      return;
    }
    for (const listener of [...this.#messageListeners])
      listener(message);
  };

  readonly #onError = (): void => this.close(1011, 'Relay connection failed');
  readonly #onRemoteClose = (): void => this.#terminate();

  #terminate(): void {
    if (this.#terminal)
      return;
    this.#terminal = true;
    this.#opened = false;
    if (this.#heartbeat !== undefined) {
      this.timers.clearInterval(this.#heartbeat);
      this.#heartbeat = undefined;
    }
    this.#clearHeartbeatDeadline();
    this.#relay = undefined;
    this.socket.removeEventListener('open', this.#onOpen);
    this.socket.removeEventListener('message', this.#onMessage);
    this.socket.removeEventListener('error', this.#onError);
    this.socket.removeEventListener('close', this.#onRemoteClose);
    this.onTerminal();
    for (const listener of [...this.#closeListeners])
      listener();
    this.#closeListeners.clear();
    this.#messageListeners.clear();
  }

  #clearHeartbeatDeadline(): void {
    if (this.#heartbeatDeadline === undefined)
      return;
    this.timers.clearTimeout(this.#heartbeatDeadline);
    this.#heartbeatDeadline = undefined;
  }
}

function isPairingHandoff(value: RelayPairingHandoff): boolean {
  return typeof value?.endpoint === 'string' && typeof value?.token === 'string' && value.token.length > 0;
}

export function isNumericLoopbackWebSocket(endpoint: string): boolean {
  let url: URL;
  try { url = new URL(endpoint); } catch { return false; }
  if (url.protocol !== 'ws:' || url.username || url.password || url.search || url.hash || !url.port)
    return false;
  if (url.hostname === '[::1]')
    return true;
  return url.hostname === '127.0.0.1';
}
