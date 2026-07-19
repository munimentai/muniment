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
  onRemoved: { addListener(listener: (tabId: number) => void): void };
}

export interface ChromeRuntime {
  getURL(path: string): string;
  onMessage: {
    addListener(listener: (message: unknown) => boolean | void | Promise<unknown>): void;
  };
  onConnect: {
    addListener(listener: (port: ChromePort) => void): void;
  };
  onSuspend: { addListener(listener: () => void): void };
}

export interface ChromePort {
  name: string;
  onDisconnect: { addListener(listener: () => void): void };
}

export interface ExtensionChrome {
  tabs: ChromeTabs;
  runtime: ChromeRuntime;
}

export class AnchorLifecycle {
  readonly #chrome: ExtensionChrome;
  readonly #anchorBaseUrl: string;
  #ownedTabId: number | undefined;
  #operation = Promise.resolve<unknown>(undefined);

  constructor(chrome: ExtensionChrome) {
    this.#chrome = chrome;
    this.#anchorBaseUrl = chrome.runtime.getURL('anchor.html');
  }

  start(): Promise<void> {
    this.#chrome.tabs.onRemoved.addListener(tabId => {
      if (tabId === this.#ownedTabId)
        void this.teardown();
    });
    return this.teardown();
  }

  connect(): Promise<number> {
    return this.#serialize(async () => {
      if (this.#ownedTabId !== undefined)
        return this.#ownedTabId;

      await this.#disconnectTaggedAnchors();
      const tab = await this.#chrome.tabs.create({
        active: true,
        url: `${this.#anchorBaseUrl}${CONNECTED_HASH}`,
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
      await this.#chrome.tabs.update(tab.id, { url: `${this.#anchorBaseUrl}${DISCONNECTED_HASH}` });
    }));
  }
}

export function isLifecycleCommand(message: unknown): message is { type: 'muniment.connect' | 'muniment.teardown' } {
  if (typeof message !== 'object' || message === null)
    return false;
  const type = (message as { type?: unknown }).type;
  return type === 'muniment.connect' || type === 'muniment.teardown';
}
