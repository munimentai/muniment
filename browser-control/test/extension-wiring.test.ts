import { readFile } from 'node:fs/promises';
import { describe, expect, it, vi } from 'vitest';

import { RelayConnection, type RelayTransport } from '../src/index.js';
import { ANCHOR_PORT, installSessionLifecycle } from '../src/extension/session-lifecycle.js';
import type { ChromePort, ChromeTab, ExtensionChrome } from '../src/extension/anchor-lifecycle.js';

class Event<T extends (...args: never[]) => void> {
  listeners: T[] = [];
  addListener = (listener: T) => { this.listeners.push(listener); };
  emit(...args: Parameters<T>): void { for (const listener of [...this.listeners]) listener(...args); }
}

class FakePort implements ChromePort {
  readonly onMessage = new Event<(message: unknown) => void>();
  readonly onDisconnect = new Event<() => void>();
  disconnected = false;
  constructor(readonly name: string) {}
  disconnect(): void {
    if (this.disconnected) return;
    this.disconnected = true;
    this.onDisconnect.emit();
  }
}

class FakeRelayTransport implements RelayTransport {
  readonly messages = new Event<(message: string | Uint8Array) => void>();
  readonly closed = new Event<() => void>();
  disconnected = false;
  send(): void {}
  close(): void { if (!this.disconnected) { this.disconnected = true; this.closed.emit(); } }
  onMessage(listener: (message: string | Uint8Array) => void): () => void {
    this.messages.addListener(listener); return () => { this.messages.listeners = this.messages.listeners.filter(item => item !== listener); };
  }
  onClose(listener: () => void): () => void {
    this.closed.addListener(listener); return () => { this.closed.listeners = this.closed.listeners.filter(item => item !== listener); };
  }
  event(method: string): void { this.messages.emit(JSON.stringify({ method })); }
}

class FakeChrome implements ExtensionChrome {
  readonly base = 'chrome-extension://cdedcfbgomnhfpifpgdlpfkkanaofkjd/';
  readonly records = new Map<number, ChromeTab>();
  readonly removed = new Event<(tabId: number) => void>();
  readonly updated = new Event<(tabId: number, changeInfo: { url?: string }) => void>();
  readonly replaced = new Event<(addedTabId: number, removedTabId: number) => void>();
  readonly connected = new Event<(port: ChromePort) => void>();
  readonly suspended = new Event<() => void>();
  nextId = 1;
  created = 0;
  failUpdates = new Set<number>();
  failRemoves = new Set<number>();
  tabs = {
    create: async ({ url }: { active: boolean; url: string }) => {
      const tab = { id: this.nextId++, url }; this.records.set(tab.id, tab); this.created++; return tab;
    },
    query: async ({ url }: { url: string }) => [...this.records.values()].filter(tab => tab.url?.startsWith(url.slice(0, -1))),
    update: async (tabId: number, { url }: { url: string }) => {
      if (this.failUpdates.has(tabId)) throw new Error('update failed');
      const tab = this.records.get(tabId); if (!tab) throw new Error('missing tab'); tab.url = url; return tab;
    },
    remove: async (tabId: number) => {
      if (this.failRemoves.has(tabId)) throw new Error('remove failed');
      if (!this.records.delete(tabId)) throw new Error('missing tab');
    },
    onRemoved: { addListener: this.removed.addListener },
    onUpdated: { addListener: this.updated.addListener },
    onReplaced: { addListener: this.replaced.addListener },
  };
  runtime = {
    getURL: (path: string) => this.base + path,
    onConnect: { addListener: this.connected.addListener },
    onSuspend: { addListener: this.suspended.addListener },
  };
  anchor(): FakePort { const port = new FakePort(ANCHOR_PORT); this.connected.emit(port); return port; }
  relay(session: ReturnType<typeof installSessionLifecycle>): { transport: FakeRelayTransport; relay: RelayConnection } {
    const transport = new FakeRelayTransport(); const relay = new RelayConnection(transport); session.attach(relay); return { transport, relay };
  }
  async settle(): Promise<void> { await new Promise(resolve => setTimeout(resolve, 0)); }
  live(): ChromeTab[] { return [...this.records.values()].filter(tab => tab.url?.endsWith('#muniment-owned-connected')); }
}

describe('MV3 session wiring', () => {
  it('installs the real service-worker entry and accepts the desktop lifecycle event', async () => {
    const chrome = new FakeChrome();
    vi.stubGlobal('chrome', chrome);
    vi.resetModules();
    const worker = await import('../src/extension/service-worker.js');
    const { authorizedRelayProvider } = await import('../src/extension/relay-provider.js');
    await chrome.settle();
    const transport = new FakeRelayTransport();
    authorizedRelayProvider.accept(new RelayConnection(transport));
    transport.event('muniment.connect'); await chrome.settle();
    expect(chrome.live()).toHaveLength(1);
    const id = chrome.live()[0]!.id!;
    chrome.records.delete(id); chrome.removed.emit(id); await chrome.settle();
    expect(transport.disconnected).toBe(true); expect(chrome.live()).toHaveLength(0);

    const replacement = new FakeRelayTransport();
    authorizedRelayProvider.accept(new RelayConnection(replacement));
    replacement.event('muniment.connect'); await chrome.settle();
    expect(chrome.live()).toHaveLength(1);
    replacement.close(); await chrome.settle();
    expect(chrome.live()).toHaveLength(0);
    await worker.sessionLifecycle.teardown();
    vi.unstubAllGlobals();
  });

  it('drives first/duplicate connect and teardown from the desktop relay', async () => {
    const chrome = new FakeChrome(); const session = installSessionLifecycle(chrome); await session.ready;
    const { transport } = chrome.relay(session);
    transport.event('muniment.connect'); transport.event('muniment.connect'); await chrome.settle();
    expect(chrome.created).toBe(1); expect(chrome.live()).toHaveLength(1);
    transport.event('muniment.teardown'); await chrome.settle();
    expect(transport.disconnected).toBe(true); expect(chrome.live()).toHaveLength(0);
  });

  it.each(['close', 'navigate', 'replace'] as const)('ends the relay session on anchor %s and permits a raced reconnect only after cleanup', async action => {
    const chrome = new FakeChrome(); const session = installSessionLifecycle(chrome); await session.ready;
    const { transport } = chrome.relay(session); transport.event('muniment.connect'); await chrome.settle();
    const id = chrome.live()[0]!.id!;
    if (action === 'close') { chrome.records.delete(id); chrome.removed.emit(id); }
    if (action === 'navigate') { chrome.records.get(id)!.url = 'https://example.com'; chrome.updated.emit(id, { url: 'https://example.com' }); }
    if (action === 'replace') chrome.replaced.emit(99, id);
    transport.event('muniment.connect'); await chrome.settle();
    expect(transport.disconnected).toBe(true); expect(chrome.live()).toHaveLength(0);
    const nextRelay = chrome.relay(session); nextRelay.transport.event('muniment.connect'); await chrome.settle();
    expect(chrome.live()).toHaveLength(1);
  });

  it('recovers stale anchors, tears down on heartbeat/relay loss, and restarts disconnected', async () => {
    const chrome = new FakeChrome(); chrome.records.set(8, { id: 8, url: chrome.base + 'anchor.html#muniment-owned-connected' });
    const first = installSessionLifecycle(chrome); await first.ready; expect(chrome.live()).toHaveLength(0);
    const firstRelay = chrome.relay(first); firstRelay.transport.event('muniment.connect'); await chrome.settle();
    chrome.anchor().disconnect(); await chrome.settle(); expect(firstRelay.transport.disconnected).toBe(true); expect(chrome.live()).toHaveLength(0);
    const relay2 = chrome.relay(first); relay2.transport.event('muniment.connect'); await chrome.settle(); relay2.transport.close(); await chrome.settle();
    expect(chrome.live()).toHaveLength(0);
    const replacement = installSessionLifecycle(chrome); await replacement.ready; expect(chrome.live()).toHaveLength(0);
  });

  it('synchronously closes the relay on suspend before asynchronous tab cleanup', async () => {
    const chrome = new FakeChrome(); const session = installSessionLifecycle(chrome); await session.ready;
    const { transport } = chrome.relay(session); transport.event('muniment.connect'); await chrome.settle();
    chrome.suspended.emit(); expect(transport.disconnected).toBe(true); await chrome.settle(); expect(chrome.live()).toHaveLength(0);
  });

  it('closes an anchor when its disconnected rewrite fails', async () => {
    const chrome = new FakeChrome(); const errors: unknown[] = []; const session = installSessionLifecycle(chrome, error => errors.push(error)); await session.ready;
    const { transport } = chrome.relay(session); transport.event('muniment.connect'); await chrome.settle();
    const id = chrome.live()[0]!.id!; chrome.failUpdates.add(id); transport.close(); await chrome.settle();
    expect(chrome.records.has(id)).toBe(false); expect(chrome.live()).toHaveLength(0); expect(errors).toEqual([]);
  });

  it('tolerates a tab disappearing during rewrite fallback and reports a surviving connected marker', async () => {
    const chrome = new FakeChrome(); const errors: unknown[] = []; const session = installSessionLifecycle(chrome, error => errors.push(error)); await session.ready;
    const first = chrome.relay(session); first.transport.event('muniment.connect'); await chrome.settle();
    const id = chrome.live()[0]!.id!; chrome.failUpdates.add(id); chrome.records.delete(id); first.transport.close(); await chrome.settle();
    expect(errors).toEqual([]); expect(chrome.live()).toHaveLength(0);

    const second = chrome.relay(session); second.transport.event('muniment.connect'); await chrome.settle();
    const surviving = chrome.live()[0]!.id!; chrome.failUpdates.add(surviving); chrome.failRemoves.add(surviving); second.transport.close(); await chrome.settle();
    expect(errors).toHaveLength(1); expect(String(errors[0])).toContain('Unable to confirm');
  });
});

describe('anchor page fallback', () => {
  it('renders exact connected copy and durably marks worker-port loss disconnected', async () => {
    const script = await readFile(new URL('../anchor.js', import.meta.url), 'utf8');
    const status = { textContent: '' }; const label = { textContent: '' }; const body = { dataset: { state: '' } };
    const disconnect = new Event<() => void>(); const replaceState = vi.fn((_a, _b, hash: string) => { location.hash = hash; });
    const location = { hash: '#muniment-owned-connected' };
    const run = new Function('document', 'location', 'history', 'chrome', script);
    run({ body, querySelector: (selector: string) => selector === '#status' ? status : label }, location, { replaceState }, {
      runtime: { connect: () => ({ onDisconnect: { addListener: disconnect.addListener } }) },
    });
    expect(status.textContent).toBe('Muniment browser control is connected. Keep this tab open; closing it disconnects browser control.');
    expect(body.dataset.state).toBe('connected');
    disconnect.emit();
    expect(location.hash).toBe('#muniment-owned-disconnected');
    expect(status.textContent).toBe('Browser control disconnected. Reconnect from the Muniment desktop app.');
    expect(body.dataset.state).toBe('disconnected');
  });

  it('keeps the required copy and responsive light/dark treatment in the page', async () => {
    const html = await readFile(new URL('../anchor.html', import.meta.url), 'utf8');
    expect(html).toContain('Browser control disconnected. Reconnect from the Muniment desktop app.');
    expect(html).toContain('prefers-color-scheme: dark');
    expect(html).toContain('width: min(100%, 560px)');
  });
});
