import { describe, expect, it, vi } from 'vitest';

import { HEARTBEAT_ACK_TIMEOUT_MS, HEARTBEAT_INTERVAL_MS, LoopbackRelayAdapter, PAIRING_PROTOCOL, type RelayWebSocket } from '../src/extension/loopback-relay.js';
import { AuthorizedRelayProvider } from '../src/extension/relay-provider.js';

type SocketEvent = 'open' | 'message' | 'error' | 'close';

class FakeSocket implements RelayWebSocket {
  readyState = 0;
  protocol = PAIRING_PROTOCOL;
  binaryType: BinaryType = 'blob';
  readonly sent: string[] = [];
  readonly closes: Array<[number | undefined, string | undefined]> = [];
  failSend = false;
  readonly listeners = new Map<SocketEvent, Set<(event: never) => void>>();

  send(data: string): void {
    if (this.failSend) throw new Error('send failed');
    this.sent.push(data);
  }
  close(code?: number, reason?: string): void { this.closes.push([code, reason]); this.readyState = 3; }
  addEventListener(type: SocketEvent, listener: (event: never) => void): void {
    const listeners = this.listeners.get(type) ?? new Set(); listeners.add(listener); this.listeners.set(type, listeners);
  }
  removeEventListener(type: SocketEvent, listener: (event: never) => void): void { this.listeners.get(type)?.delete(listener); }
  emit(type: SocketEvent, data?: unknown): void {
    if (type === 'open') this.readyState = 1;
    if (type === 'close') this.readyState = 3;
    const event = type === 'message' ? { data } : {};
    for (const listener of [...(this.listeners.get(type) ?? [])]) listener(event as never);
  }
}

function setup() {
  const provider = new AuthorizedRelayProvider();
  const sockets: FakeSocket[] = [];
  const calls: Array<{ endpoint: string; protocols: string[] }> = [];
  const adapter = new LoopbackRelayAdapter(provider, (endpoint, protocols) => {
    calls.push({ endpoint, protocols }); const socket = new FakeSocket(); sockets.push(socket); return socket;
  });
  return { provider, sockets, calls, adapter };
}

describe('loopback relay handoff', () => {
  it('uses the pairing subprotocol and composes one open socket with text, binary, send, and close mapping', async () => {
    const { provider, sockets, calls, adapter } = setup();
    const accepted: Parameters<typeof provider.accept>[0][] = [];
    provider.subscribe(relay => accepted.push(relay));
    expect(adapter.accept({ endpoint: 'ws://127.0.0.1:43123/browser', token: 'single-use-secret' })).toBe(true);
    expect(calls).toEqual([{ endpoint: 'ws://127.0.0.1:43123/browser', protocols: [PAIRING_PROTOCOL, 'single-use-secret'] }]);
    expect(sockets[0]!.binaryType).toBe('arraybuffer');
    sockets[0]!.emit('open'); sockets[0]!.emit('open');
    expect(accepted).toHaveLength(1);

    const events: string[] = [];
    accepted[0]!.onEvent(event => events.push(event.method));
    sockets[0]!.emit('message', JSON.stringify({ method: 'text.event' }));
    sockets[0]!.emit('message', new TextEncoder().encode(JSON.stringify({ method: 'binary.event' })).buffer);
    expect(events).toEqual(['text.event', 'binary.event']);
    const pending = accepted[0]!.send('desktop.request');
    expect(sockets[0]!.sent.at(-1)).toContain('desktop.request');
    sockets[0]!.emit('message', JSON.stringify({ id: 1, result: 'ok' }));
    await expect(pending).resolves.toBe('ok');
    accepted[0]!.close();
    expect(sockets[0]!.closes).toEqual([[1000, 'Relay closed']]);
  });

  it.each([
    'wss://127.0.0.1:4/x', 'http://127.0.0.1:4/x', 'ws://localhost:4/x',
    'ws://127.0.0.2:4/x', 'ws://[::2]:4/x', 'ws://127.0.0.1/x',
    'ws://user@127.0.0.1:4/x', 'ws://127.0.0.1:4/x?token=bad', 'not a url',
  ])('rejects invalid endpoint %s without opening a socket', endpoint => {
    const { adapter, calls } = setup();
    expect(adapter.accept({ endpoint, token: 'secret' })).toBe(false);
    expect(calls).toEqual([]);
  });

  it('accepts IPv6 loopback and rejects empty pairing material', () => {
    const { adapter, calls } = setup();
    expect(adapter.accept({ endpoint: 'ws://[::1]:43123/browser', token: 'token' })).toBe(true);
    expect(adapter.accept({ endpoint: 'ws://127.0.0.1:43123/browser', token: '' })).toBe(false);
    expect(calls).toHaveLength(1);
  });

  it('keeps handoff secrets out of URLs, logging, and persistence', () => {
    const log = vi.spyOn(console, 'log').mockImplementation(() => {});
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const storageSet = vi.fn();
    vi.stubGlobal('chrome', { storage: { local: { set: storageSet } } });
    const { adapter, calls } = setup();
    adapter.accept({ endpoint: 'ws://127.0.0.1:43123/browser', token: 'never-disclose-this' });
    expect(calls[0]!.endpoint).not.toContain('never-disclose-this');
    expect(log).not.toHaveBeenCalled(); expect(error).not.toHaveBeenCalled(); expect(storageSet).not.toHaveBeenCalled();
    vi.unstubAllGlobals(); log.mockRestore(); error.mockRestore();
  });

  it('closes superseded and raced attempts and never authorizes them', () => {
    const { provider, sockets, adapter } = setup(); const accepted = vi.fn(); provider.subscribe(accepted);
    adapter.accept({ endpoint: 'ws://127.0.0.1:1/a', token: 'first' });
    adapter.accept({ endpoint: 'ws://127.0.0.1:2/b', token: 'second' });
    expect(sockets[0]!.closes).toEqual([[1000, 'Relay revoked']]);
    sockets[0]!.emit('open'); expect(accepted).not.toHaveBeenCalled();
    sockets[1]!.emit('open'); expect(accepted).toHaveBeenCalledOnce();
  });

  it.each(['connecting', 'open'] as const)('revokes a %s relay before rejecting an invalid superseding handoff', state => {
    const { provider, sockets, calls, adapter } = setup(); const accepted = vi.fn(); provider.subscribe(accepted);
    adapter.accept({ endpoint: 'ws://127.0.0.1:1/a', token: 'first' });
    if (state === 'open') sockets[0]!.emit('open');
    expect(adapter.accept({ endpoint: 'ws://localhost:2/b', token: 'second' })).toBe(false);
    expect(sockets[0]!.closes).toEqual([[1000, 'Relay revoked']]);
    expect(calls).toHaveLength(1);
    sockets[0]!.emit('open');
    expect(accepted).toHaveBeenCalledTimes(state === 'open' ? 1 : 0);
  });

  it.each(['connecting', 'open'] as const)('revokes a %s relay before rejecting an empty-token superseding handoff', state => {
    const { provider, sockets, calls, adapter } = setup(); const accepted = vi.fn(); provider.subscribe(accepted);
    adapter.accept({ endpoint: 'ws://127.0.0.1:1/a', token: 'first' });
    if (state === 'open') sockets[0]!.emit('open');
    expect(adapter.accept({ endpoint: 'ws://127.0.0.1:2/b', token: '' })).toBe(false);
    expect(sockets[0]!.closes).toEqual([[1000, 'Relay revoked']]);
    expect(calls).toHaveLength(1);
    sockets[0]!.emit('open');
    expect(accepted).toHaveBeenCalledTimes(state === 'open' ? 1 : 0);
  });

  it('keeps the worker alive inside 30 seconds and terminates on heartbeat failure without reconnecting', async () => {
    vi.useFakeTimers();
    const { provider, sockets, calls, adapter } = setup(); const accepted = vi.fn(); provider.subscribe(accepted);
    adapter.accept({ endpoint: 'ws://127.0.0.1:1/a', token: 'secret' }); sockets[0]!.emit('open');
    await vi.advanceTimersByTimeAsync(HEARTBEAT_INTERVAL_MS);
    expect(sockets[0]!.sent).toEqual([JSON.stringify({ id: 1, method: 'muniment.heartbeat' })]);
    sockets[0]!.emit('message', JSON.stringify({ id: 1, result: null }));
    await vi.advanceTimersByTimeAsync(HEARTBEAT_INTERVAL_MS);
    expect(sockets[0]!.sent).toHaveLength(2);
    sockets[0]!.emit('message', JSON.stringify({ id: 2, result: null }));
    sockets[0]!.failSend = true; await vi.advanceTimersByTimeAsync(HEARTBEAT_INTERVAL_MS);
    expect(sockets[0]!.closes).toEqual([[1011, 'Relay transport failure']]);
    vi.advanceTimersByTime(HEARTBEAT_INTERVAL_MS * 2);
    expect(calls).toHaveLength(1); expect(vi.getTimerCount()).toBe(0);
    vi.useRealTimers();
  });

  it('terminates when heartbeat sends succeed but no acknowledgement arrives', () => {
    vi.useFakeTimers();
    const { provider, sockets, calls, adapter } = setup(); provider.subscribe(vi.fn());
    adapter.accept({ endpoint: 'ws://127.0.0.1:1/a', token: 'secret' }); sockets[0]!.emit('open');
    vi.advanceTimersByTime(HEARTBEAT_INTERVAL_MS);
    expect(sockets[0]!.sent).toEqual([JSON.stringify({ id: 1, method: 'muniment.heartbeat' })]);
    vi.advanceTimersByTime(HEARTBEAT_ACK_TIMEOUT_MS);
    expect(sockets[0]!.closes).toEqual([[1011, 'Heartbeat acknowledgement timed out']]);
    vi.advanceTimersByTime(HEARTBEAT_INTERVAL_MS * 2);
    expect(calls).toHaveLength(1); expect(vi.getTimerCount()).toBe(0);
    vi.useRealTimers();
  });

  it('fails closed on protocol mismatch, connection error, remote close, and explicit teardown', () => {
    for (const terminal of ['protocol', 'error', 'remote', 'teardown'] as const) {
      const { provider, sockets, adapter } = setup(); const accepted = vi.fn(); provider.subscribe(accepted);
      adapter.accept({ endpoint: 'ws://127.0.0.1:1/a', token: 'secret' });
      if (terminal === 'protocol') sockets[0]!.protocol = '';
      if (terminal === 'error') sockets[0]!.emit('error');
      else if (terminal === 'remote') { sockets[0]!.emit('open'); sockets[0]!.emit('close'); }
      else if (terminal === 'teardown') adapter.close();
      else sockets[0]!.emit('open');
      expect(accepted).toHaveBeenCalledTimes(terminal === 'remote' ? 1 : 0);
      if (terminal !== 'remote') expect(sockets[0]!.closes).toHaveLength(1);
    }
  });
});
