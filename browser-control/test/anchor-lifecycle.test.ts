import { describe, expect, it } from 'vitest';

import {
  AnchorLifecycle,
  CONNECTED_COPY,
  DISCONNECTED_COPY,
  type ChromePort,
  type ChromeTab,
  type ExtensionChrome,
} from '../src/extension/anchor-lifecycle.js';

class FakeChrome implements ExtensionChrome {
  readonly extensionBase = 'chrome-extension://cdedcfbgomnhfpifpgdlpfkkanaofkjd/';
  readonly records = new Map<number, ChromeTab>();
  readonly removedListeners = new Set<(tabId: number) => void>();
  readonly updatedListeners = new Set<(tabId: number, changeInfo: { url?: string }) => void>();
  readonly replacedListeners = new Set<(addedTabId: number, removedTabId: number) => void>();
  readonly connectListeners = new Set<(port: ChromePort) => void>();
  readonly suspendListeners = new Set<() => void>();
  nextId = 1;
  created: Array<{ active: boolean; url: string }> = [];
  updated: Array<{ tabId: number; url: string }> = [];

  tabs = {
    create: async (properties: { active: boolean; url: string }) => {
      this.created.push(properties);
      const tab = { id: this.nextId++, url: properties.url };
      this.records.set(tab.id, tab);
      return tab;
    },
    query: async ({ url }: { url: string }) => {
      const prefix = url.endsWith('*') ? url.slice(0, -1) : url;
      return [...this.records.values()].filter(tab => tab.url?.startsWith(prefix));
    },
    update: async (tabId: number, { url }: { url: string }) => {
      const tab = this.records.get(tabId);
      if (!tab)
        throw new Error('missing tab');
      tab.url = url;
      this.updated.push({ tabId, url });
      return tab;
    },
    remove: async (tabId: number) => { this.records.delete(tabId); },
    onRemoved: { addListener: (listener: (tabId: number) => void) => this.removedListeners.add(listener) },
    onUpdated: { addListener: (listener: (tabId: number, changeInfo: { url?: string }) => void) => this.updatedListeners.add(listener) },
    onReplaced: { addListener: (listener: (addedTabId: number, removedTabId: number) => void) => this.replacedListeners.add(listener) },
  };

  runtime = {
    getURL: (path: string) => `${this.extensionBase}${path}`,
    onConnect: { addListener: (listener: (port: ChromePort) => void) => this.connectListeners.add(listener) },
    onSuspend: { addListener: (listener: () => void) => this.suspendListeners.add(listener) },
  };

  addUserTab(url: string): number {
    const id = this.nextId++;
    this.records.set(id, { id, url });
    return id;
  }

  close(tabId: number): void {
    this.records.delete(tabId);
    for (const listener of this.removedListeners)
      listener(tabId);
  }
}

const connectedUrl = (chrome: FakeChrome) => `${chrome.extensionBase}anchor.html#muniment-owned-connected`;
const disconnectedUrl = (chrome: FakeChrome) => `${chrome.extensionBase}anchor.html#muniment-owned-disconnected`;

describe('AnchorLifecycle', () => {
  it('creates one ordinary visible anchor on first connect and keeps the exact copy', async () => {
    const chrome = new FakeChrome();
    const lifecycle = new AnchorLifecycle(chrome);
    await lifecycle.start();

    const id = await lifecycle.connect();

    expect(chrome.created).toEqual([{ active: true, url: connectedUrl(chrome) }]);
    expect(chrome.records.get(id)?.url).toBe(connectedUrl(chrome));
    expect(CONNECTED_COPY).toBe('Muniment browser control is connected. Keep this tab open; closing it disconnects browser control.');
    expect(DISCONNECTED_COPY).toBe('Browser control disconnected. Reconnect from the Muniment desktop app.');
  });

  it('coalesces duplicate and concurrent connect commands onto its owned anchor', async () => {
    const chrome = new FakeChrome();
    const lifecycle = new AnchorLifecycle(chrome);
    await lifecycle.start();

    const [first, duplicate] = await Promise.all([lifecycle.connect(), lifecycle.connect()]);

    expect(duplicate).toBe(first);
    expect(chrome.created).toHaveLength(1);
  });

  it('disconnects tagged stale anchors before creating one new anchor without adopting user tabs', async () => {
    const chrome = new FakeChrome();
    const stale = chrome.addUserTab(connectedUrl(chrome));
    const unrelated = chrome.addUserTab('https://example.com/');
    const lifecycle = new AnchorLifecycle(chrome);
    await lifecycle.start();

    await lifecycle.connect();

    expect(chrome.records.get(stale)?.url).toBe(disconnectedUrl(chrome));
    expect(chrome.records.get(unrelated)?.url).toBe('https://example.com/');
    expect([...chrome.records.values()].filter(tab => tab.url === connectedUrl(chrome))).toHaveLength(1);
  });

  it('cleans up idempotently when the owned anchor closes', async () => {
    const chrome = new FakeChrome();
    const lifecycle = new AnchorLifecycle(chrome);
    await lifecycle.start();
    const id = await lifecycle.connect();

    chrome.close(id);
    await lifecycle.teardown();
    await lifecycle.teardown();

    expect([...chrome.records.values()].filter(tab => tab.url === connectedUrl(chrome))).toHaveLength(0);
  });

  it('marks the surviving anchor disconnected on relay or session teardown', async () => {
    const chrome = new FakeChrome();
    const lifecycle = new AnchorLifecycle(chrome);
    await lifecycle.start();
    const id = await lifecycle.connect();

    await lifecycle.teardown();

    expect(chrome.records.get(id)?.url).toBe(disconnectedUrl(chrome));
  });

  it('starts a replacement worker disconnected and does not silently resume', async () => {
    const chrome = new FakeChrome();
    const firstWorker = new AnchorLifecycle(chrome);
    await firstWorker.start();
    const oldAnchor = await firstWorker.connect();

    const replacementWorker = new AnchorLifecycle(chrome);
    await replacementWorker.start();

    expect(chrome.records.get(oldAnchor)?.url).toBe(disconnectedUrl(chrome));
    expect(chrome.created).toHaveLength(1);
  });
});
