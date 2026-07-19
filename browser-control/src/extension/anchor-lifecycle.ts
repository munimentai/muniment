export const CONNECTED_COPY = 'Muniment browser control is connected. Keep this tab open; closing it disconnects browser control.';
export const DISCONNECTED_COPY = 'Browser control disconnected. Reconnect from the Muniment desktop app.';

const CONNECTED_HASH = '#muniment-owned-connected';
const DISCONNECTED_HASH = '#muniment-owned-disconnected';

export interface ChromeTab {
  id?: number;
  url?: string;
}

export interface ChromeTabs {
  create(properties: { active: boolean; url: string }): Promise<ChromeTab>;
  query(queryInfo: { url: string }): Promise<ChromeTab[]>;
  update(tabId: number, properties: { url: string }): Promise<ChromeTab>;
  remove(tabId: number): Promise<void>;
  onRemoved: { addListener(listener: (tabId: number) => void): void };
  onUpdated: { addListener(listener: (tabId: number, changeInfo: { url?: string }) => void): void };
  onReplaced: { addListener(listener: (addedTabId: number, removedTabId: number) => void): void };
}

export interface ChromeRuntime {
  getURL(path: string): string;
  onConnect: {
    addListener(listener: (port: ChromePort) => void): void;
  };
  onSuspend: { addListener(listener: () => void): void };
}

export interface ChromePort {
  name: string;
  onMessage: { addListener(listener: (message: unknown) => void): void };
  onDisconnect: { addListener(listener: () => void): void };
  disconnect(): void;
}

export interface ExtensionChrome {
  tabs: ChromeTabs;
  runtime: ChromeRuntime;
}

export class AnchorLifecycle {
  readonly #chrome: ExtensionChrome;
  readonly #anchorBaseUrl: string;
  readonly #onAnchorLost: () => void;
  #ownedTabId: number | undefined;
  #operation = Promise.resolve<unknown>(undefined);

  constructor(chrome: ExtensionChrome, onAnchorLost: () => void = () => {}) {
    this.#chrome = chrome;
    this.#anchorBaseUrl = chrome.runtime.getURL('anchor.html');
    this.#onAnchorLost = onAnchorLost;
  }

  start(): Promise<void> {
    this.#chrome.tabs.onRemoved.addListener(tabId => {
      if (tabId === this.#ownedTabId)
        this.#onAnchorLost();
    });
    this.#chrome.tabs.onUpdated.addListener((tabId, changeInfo) => {
      if (tabId === this.#ownedTabId && changeInfo.url !== undefined && changeInfo.url !== this.connectedUrl)
        this.#onAnchorLost();
    });
    this.#chrome.tabs.onReplaced.addListener((_addedTabId, removedTabId) => {
      if (removedTabId === this.#ownedTabId)
        this.#onAnchorLost();
    });
    return this.teardown();
  }

  get connectedUrl(): string {
    return `${this.#anchorBaseUrl}${CONNECTED_HASH}`;
  }

  connect(): Promise<number> {
    return this.#serialize(async () => {
      if (this.#ownedTabId !== undefined)
        return this.#ownedTabId;

      await this.#disconnectTaggedAnchors();
      const tab = await this.#chrome.tabs.create({
        active: true,
        url: this.connectedUrl,
      });
      if (tab.id === undefined)
        throw new Error('Chrome did not assign the Muniment anchor a tab id');
      this.#ownedTabId = tab.id;
      return tab.id;
    });
  }

  teardown(): Promise<void> {
    return this.#serialize(async () => {
      this.#ownedTabId = undefined;
      await this.#disconnectTaggedAnchors();
    });
  }

  #serialize<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.#operation.then(operation, operation);
    this.#operation = result.catch(() => undefined);
    return result;
  }

  async #disconnectTaggedAnchors(): Promise<void> {
    const tabs = await this.#chrome.tabs.query({ url: `${this.#anchorBaseUrl}*` });
    await Promise.all(tabs.map(async tab => {
      if (tab.id === undefined)
        return;
      try {
        await this.#chrome.tabs.update(tab.id, { url: `${this.#anchorBaseUrl}${DISCONNECTED_HASH}` });
      } catch {
        try {
          await this.#chrome.tabs.remove(tab.id);
        } catch {
          // The tab may have disappeared between query, update, and remove.
        }
      }
    }));
    const survivors = (await this.#chrome.tabs.query({ url: `${this.#anchorBaseUrl}*` }))
      .filter(tab => tab.url === this.connectedUrl && tab.id !== undefined);
    await Promise.all(survivors.map(async tab => {
      try {
        await this.#chrome.tabs.remove(tab.id!);
      } catch {
        // A final query distinguishes a benign disappearance from a survivor.
      }
    }));
    const remaining = await this.#chrome.tabs.query({ url: `${this.#anchorBaseUrl}*` });
    if (remaining.some(tab => tab.url === this.connectedUrl))
      throw new Error('Unable to confirm Muniment anchor disconnection');
  }
}

export function isLifecycleCommand(message: unknown): message is { type: 'muniment.connect' | 'muniment.teardown' } {
  if (typeof message !== 'object' || message === null)
    return false;
  const type = (message as { type?: unknown }).type;
  return type === 'muniment.connect' || type === 'muniment.teardown';
}
