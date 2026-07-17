import { describe, expect, it, vi } from 'vitest';

import { RelayConnection, type RelayTransport } from '../src/index.js';

class MemoryTransport implements RelayTransport {
  sent: string[] = [];
  close = vi.fn();
  messageListeners = new Set<(message: string | Uint8Array) => void>();
  closeListeners = new Set<() => void>();

  send(message: string): void {
    this.sent.push(message);
  }

  onMessage(listener: (message: string | Uint8Array) => void): () => void {
    this.messageListeners.add(listener);
    return () => this.messageListeners.delete(listener);
  }

  onClose(listener: () => void): () => void {
    this.closeListeners.add(listener);
    return () => this.closeListeners.delete(listener);
  }

  receive(message: string | Uint8Array): void {
    for (const listener of [...this.messageListeners])
      listener(message);
  }

  disconnect(): void {
    for (const listener of [...this.closeListeners])
      listener();
  }
}

describe('RelayConnection', () => {
  it('forwards a CDP request, response, and event through the injected transport', async () => {
    const transport = new MemoryTransport();
    const relay = new RelayConnection(transport);
    const onEvent = vi.fn();
    relay.onEvent(onEvent);

    const response = relay.send('Runtime.evaluate', { expression: '2 + 2' });
    expect(JSON.parse(transport.sent[0]!)).toEqual({
      id: 1,
      method: 'Runtime.evaluate',
      params: { expression: '2 + 2' },
    });
    transport.receive(JSON.stringify({ id: 1, result: { result: { value: 4 } } }));
    await expect(response).resolves.toEqual({ result: { value: 4 } });

    transport.receive(JSON.stringify({ method: 'Runtime.consoleAPICalled', params: { type: 'log' } }));
    expect(onEvent).toHaveBeenCalledWith({ method: 'Runtime.consoleAPICalled', params: { type: 'log' } });
  });

  it.each([
    ['malformed JSON', '{'],
    ['invalid shape', '[]'],
    ['contradictory response', JSON.stringify({ id: 1, result: {}, error: 'failure' })],
  ])('rejects %s deterministically', async (_name, message) => {
    const transport = new MemoryTransport();
    const relay = new RelayConnection(transport);
    const pending = relay.send('Page.enable');

    transport.receive(message);

    expect(transport.close).toHaveBeenCalledWith(1002, 'Invalid relay message');
    await expect(pending).rejects.toThrow('Relay transport closed');
  });

  it('rejects oversized UTF-8 messages at the configured byte boundary', () => {
    const transport = new MemoryTransport();
    new RelayConnection(transport, { maxMessageBytes: 4 });

    transport.receive('\u00e9\u00e9a');

    expect(transport.close).toHaveBeenCalledWith(1002, 'Invalid relay message');
  });

  it('still disposes if the transport throws while closing invalid input', async () => {
    const transport = new MemoryTransport();
    transport.close.mockImplementation(() => { throw new Error('transport internals'); });
    const relay = new RelayConnection(transport);
    const pending = relay.send('Page.enable');

    expect(() => transport.receive('{')).not.toThrow();

    await expect(pending).rejects.toThrow('Relay transport closed');
    expect(transport.messageListeners).toHaveLength(0);
    expect(transport.closeListeners).toHaveLength(0);
  });

  it('redacts transport errors when explicitly closed', async () => {
    const transport = new MemoryTransport();
    transport.close.mockImplementation(() => { throw new Error('transport internals'); });
    const relay = new RelayConnection(transport);
    const pending = relay.send('Page.enable');

    expect(() => relay.close()).not.toThrow();

    await expect(pending).rejects.toThrow('Relay transport closed');
    expect(transport.messageListeners).toHaveLength(0);
    expect(transport.closeListeners).toHaveLength(0);
  });

  it('treats send failure as terminal and rejects every pending request', async () => {
    const transport = new MemoryTransport();
    const relay = new RelayConnection(transport);
    const first = relay.send('Page.enable');
    transport.send = () => { throw new Error('private send failure'); };

    const second = relay.send('Runtime.enable');

    await expect(first).rejects.toThrow('Relay transport closed');
    await expect(second).rejects.toThrow('Relay transport closed');
    await expect(second).rejects.not.toThrow('private send failure');
    expect(transport.close).toHaveBeenCalledWith(1011, 'Relay transport failure');
    expect(transport.messageListeners).toHaveLength(0);
    expect(transport.closeListeners).toHaveLength(0);
  });

  it('removes listeners when the transport closes during subscription', async () => {
    const transport = new MemoryTransport();
    transport.onClose = listener => {
      transport.closeListeners.add(listener);
      listener();
      return () => transport.closeListeners.delete(listener);
    };

    const relay = new RelayConnection(transport);

    expect(transport.messageListeners).toHaveLength(0);
    expect(transport.closeListeners).toHaveLength(0);
    await expect(relay.send('Page.enable')).rejects.toThrow('Relay transport closed');
  });

  it('rolls back listeners when transport subscription partially fails', () => {
    const transport = new MemoryTransport();
    transport.onClose = () => { throw new Error('subscription failure'); };

    expect(() => new RelayConnection(transport)).toThrow('subscription failure');
    expect(transport.messageListeners).toHaveLength(0);
    expect(transport.closeListeners).toHaveLength(0);
  });

  it('rejects pending requests and removes every listener on transport closure', async () => {
    const transport = new MemoryTransport();
    const relay = new RelayConnection(transport);
    const eventListener = vi.fn();
    relay.onEvent(eventListener);
    const pending = relay.send('Runtime.evaluate');

    transport.disconnect();

    await expect(pending).rejects.toThrow('Relay transport closed');
    expect(transport.messageListeners).toHaveLength(0);
    expect(transport.closeListeners).toHaveLength(0);
    transport.receive(JSON.stringify({ method: 'Runtime.event', params: {} }));
    expect(eventListener).not.toHaveBeenCalled();
    await expect(relay.send('Runtime.evaluate')).rejects.toThrow('Relay transport closed');
  });

  it('redacts remote errors', async () => {
    const transport = new MemoryTransport();
    const relay = new RelayConnection(transport);
    const pending = relay.send('Runtime.evaluate');

    transport.receive(JSON.stringify({ id: 1, error: 'secret upstream stack and filesystem path' }));

    await expect(pending).rejects.toThrow('Relay request failed');
    await expect(pending).rejects.not.toThrow('secret upstream stack');
  });

  it('ignores stale responses without disturbing a pending request', async () => {
    const transport = new MemoryTransport();
    const relay = new RelayConnection(transport);
    const pending = relay.send('Page.enable');

    transport.receive(JSON.stringify({ id: 99, result: { stale: true } }));
    transport.receive(JSON.stringify({ id: 1, result: { accepted: true } }));

    await expect(pending).resolves.toEqual({ accepted: true });
  });

  it('does not let a throwing consumer event listener close the relay', async () => {
    const transport = new MemoryTransport();
    const relay = new RelayConnection(transport);
    relay.onEvent(() => { throw new Error('consumer failure'); });

    transport.receive(JSON.stringify({ method: 'Runtime.event' }));
    const pending = relay.send('Page.enable');
    transport.receive(JSON.stringify({ id: 1, result: {} }));

    await expect(pending).resolves.toEqual({});
    expect(transport.close).not.toHaveBeenCalled();
  });

  it.each([0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1])('rejects invalid message limits: %s', maxMessageBytes => {
    expect(() => new RelayConnection(new MemoryTransport(), { maxMessageBytes })).toThrow(RangeError);
  });
});
