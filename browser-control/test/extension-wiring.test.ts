import { readFile } from 'node:fs/promises';
import { describe, expect, it, vi } from 'vitest';

import { ANCHOR_PORT, DESKTOP_RELAY_PORT, installSessionLifecycle } from '../src/extension/session-lifecycle.js';
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
  tabs = {
    create: async ({ url }: { active: boolean; url: string }) => {
      const tab = { id: this.nextId++, url }; this.records.set(tab.id, tab); this.created++; return tab;
    },
    query: async ({ url }: { url: string }) => [...this.records.values()].filter(tab => tab.url?.startsWith(url.slice(0, -1))),
    update: async (tabId: number, { url }: { url: string }) => {
      const tab = this.records.get(tabId); if (!tab) throw new Error('missing tab'); tab.url = url; return tab;
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
  relay(): FakePort { const port = new FakePort(DESKTOP_RELAY_PORT); this.connected.emit(port); return port; }
  anchor(): FakePort { const port = new FakePort(ANCHOR_PORT); this.connected.emit(port); return port; }
  async settle(): Promise<void> { await new Promise(resolve => setTimeout(resolve, 0)); }
  live(): ChromeTab[] { return [...this.records.values()].filter(tab => tab.url?.endsWith('#muniment-owned-connected')); }
}

describe('MV3 session wiring', () => {
  it('installs the real service-worker entry and accepts the desktop lifecycle event', async () => {
    const chrome = new FakeChrome();
    vi.stubGlobal('chrome', chrome);
    vi.resetModules();
    await import('../src/extension/service-worker.js');
    await chrome.settle();
    const relay = chrome.relay(); relay.onMessage.emit({ type: 'muniment.connect' }); await chrome.settle();
    expect(chrome.live()).toHaveLength(1);
    vi.unstubAllGlobals();
  });

  it('drives first/duplicate connect and teardown from the desktop relay', async () => {
    const chrome = new FakeChrome(); const session = installSessionLifecycle(chrome); await session.ready;
    const relay = chrome.relay();
    relay.onMessage.emit({ type: 'muniment.connect' }); relay.onMessage.emit({ type: 'muniment.connect' }); await chrome.settle();
    expect(chrome.created).toBe(1); expect(chrome.live()).toHaveLength(1);
    relay.onMessage.emit({ type: 'muniment.teardown' }); await chrome.settle();
    expect(relay.disconnected).toBe(true); expect(chrome.live()).toHaveLength(0);
  });

  it.each(['close', 'navigate', 'replace'] as const)('ends the relay session on anchor %s and permits a raced reconnect only after cleanup', async action => {
    const chrome = new FakeChrome(); const session = installSessionLifecycle(chrome); await session.ready;
    const relay = chrome.relay(); relay.onMessage.emit({ type: 'muniment.connect' }); await chrome.settle();
    const id = chrome.live()[0]!.id!;
    if (action === 'close') { chrome.records.delete(id); chrome.removed.emit(id); }
    if (action === 'navigate') { chrome.records.get(id)!.url = 'https://example.com'; chrome.updated.emit(id, { url: 'https://example.com' }); }
    if (action === 'replace') chrome.replaced.emit(99, id);
    relay.onMessage.emit({ type: 'muniment.connect' }); await chrome.settle();
    expect(relay.disconnected).toBe(true); expect(chrome.live()).toHaveLength(0);
    const nextRelay = chrome.relay(); nextRelay.onMessage.emit({ type: 'muniment.connect' }); await chrome.settle();
    expect(chrome.live()).toHaveLength(1);
  });

  it('recovers stale anchors, tears down on heartbeat/relay loss, and restarts disconnected', async () => {
    const chrome = new FakeChrome(); chrome.records.set(8, { id: 8, url: chrome.base + 'anchor.html#muniment-owned-connected' });
    const first = installSessionLifecycle(chrome); await first.ready; expect(chrome.live()).toHaveLength(0);
    const relay = chrome.relay(); relay.onMessage.emit({ type: 'muniment.connect' }); await chrome.settle();
    chrome.anchor().disconnect(); await chrome.settle(); expect(relay.disconnected).toBe(true); expect(chrome.live()).toHaveLength(0);
    const relay2 = chrome.relay(); relay2.onMessage.emit({ type: 'muniment.connect' }); await chrome.settle(); relay2.disconnect(); await chrome.settle();
    expect(chrome.live()).toHaveLength(0);
    const replacement = installSessionLifecycle(chrome); await replacement.ready; expect(chrome.live()).toHaveLength(0);
  });

  it('synchronously closes the relay on suspend before asynchronous tab cleanup', async () => {
    const chrome = new FakeChrome(); const session = installSessionLifecycle(chrome); await session.ready;
    const relay = chrome.relay(); relay.onMessage.emit({ type: 'muniment.connect' }); await chrome.settle();
    chrome.suspended.emit(); expect(relay.disconnected).toBe(true); await chrome.settle(); expect(chrome.live()).toHaveLength(0);
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
