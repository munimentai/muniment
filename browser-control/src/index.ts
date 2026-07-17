/**
 * Copyright (c) Microsoft Corporation.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 * http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

export const DEFAULT_MAX_MESSAGE_BYTES = 1024 * 1024;

export interface RelayTransport {
  send(message: string): void;
  close(code: number, reason: string): void;
  onMessage(listener: (message: string | Uint8Array) => void): () => void;
  onClose(listener: () => void): () => void;
}

export interface RelayEvent {
  method: string;
  params?: unknown;
}

interface RelayResponse {
  id: number;
  result?: unknown;
  error?: unknown;
}

interface PendingRequest {
  resolve(value: unknown): void;
  reject(error: Error): void;
}

const CLOSED_ERROR = 'Relay transport closed';

export class RelayConnection {
  readonly #transport: RelayTransport;
  readonly #maxMessageBytes: number;
  readonly #pending = new Map<number, PendingRequest>();
  readonly #eventListeners = new Set<(event: RelayEvent) => void>();
  #removeTransportListeners: Array<() => void> = [];
  #lastId = 0;
  #closed = false;

  constructor(transport: RelayTransport, options: { maxMessageBytes?: number } = {}) {
    const maxMessageBytes = options.maxMessageBytes ?? DEFAULT_MAX_MESSAGE_BYTES;
    if (!Number.isSafeInteger(maxMessageBytes) || maxMessageBytes <= 0)
      throw new RangeError('maxMessageBytes must be a positive safe integer');
    this.#transport = transport;
    this.#maxMessageBytes = maxMessageBytes;
    this.#removeTransportListeners = [
      transport.onMessage(message => this.#onMessage(message)),
      transport.onClose(() => this.#dispose()),
    ];
  }

  send(method: string, params?: unknown): Promise<unknown> {
    if (this.#closed)
      return Promise.reject(new Error(CLOSED_ERROR));
    if (typeof method !== 'string' || method.length === 0)
      return Promise.reject(new TypeError('method must be a non-empty string'));

    const id = ++this.#lastId;
    return new Promise((resolve, reject) => {
      this.#pending.set(id, { resolve, reject });
      try {
        this.#transport.send(JSON.stringify({ id, method, params }));
      } catch {
        this.#pending.delete(id);
        reject(new Error(CLOSED_ERROR));
      }
    });
  }

  onEvent(listener: (event: RelayEvent) => void): () => void {
    if (this.#closed)
      return () => {};
    this.#eventListeners.add(listener);
    return () => this.#eventListeners.delete(listener);
  }

  close(): void {
    if (this.#closed)
      return;
    try {
      this.#transport.close(1000, 'Relay closed');
    } finally {
      this.#dispose();
    }
  }

  #onMessage(data: string | Uint8Array): void {
    if (this.#closed)
      return;
    let text: string;
    try {
      const byteLength = typeof data === 'string' ? new TextEncoder().encode(data).byteLength : data.byteLength;
      if (byteLength > this.#maxMessageBytes)
        throw new Error('oversized');
      text = typeof data === 'string' ? data : new TextDecoder('utf-8', { fatal: true }).decode(data);
      const message: unknown = JSON.parse(text);
      if (!isRecord(message))
        throw new Error('invalid shape');

      if ('id' in message) {
        if (!isResponse(message))
          throw new Error('invalid response');
        const pending = this.#pending.get(message.id);
        if (!pending)
          return;
        this.#pending.delete(message.id);
        if ('error' in message)
          pending.reject(new Error('Relay request failed'));
        else
          pending.resolve(message.result);
        return;
      }

      if (!isEvent(message))
        throw new Error('invalid event');
      for (const listener of [...this.#eventListeners]) {
        try {
          listener({ method: message.method, ...('params' in message ? { params: message.params } : {}) });
        } catch {
          // A consumer callback cannot corrupt the relay protocol lifecycle.
        }
      }
    } catch {
      try {
        this.#transport.close(1002, 'Invalid relay message');
      } finally {
        this.#dispose();
      }
    }
  }

  #dispose(): void {
    if (this.#closed)
      return;
    this.#closed = true;
    for (const remove of this.#removeTransportListeners) {
      try {
        remove();
      } catch {
        // Continue disposing the remaining relay state.
      }
    }
    this.#removeTransportListeners = [];
    for (const pending of this.#pending.values())
      pending.reject(new Error(CLOSED_ERROR));
    this.#pending.clear();
    this.#eventListeners.clear();
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isResponse(value: Record<string, unknown>): value is Record<string, unknown> & RelayResponse {
  return Number.isSafeInteger(value.id) && (value.id as number) > 0 && (('result' in value) !== ('error' in value)) && !('method' in value);
}

function isEvent(value: Record<string, unknown>): value is Record<string, unknown> & RelayEvent {
  return typeof value.method === 'string' && value.method.length > 0 && !('result' in value) && !('error' in value);
}
