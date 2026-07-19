import { AnchorLifecycle, type ExtensionChrome } from './anchor-lifecycle.js';
import type { RelayEvent } from '../index.js';

export const ANCHOR_PORT = 'muniment-anchor-lifecycle';

export interface LifecycleRelay {
  onEvent(listener: (event: RelayEvent) => void): () => void;
  onClose(listener: () => void): () => void;
  close(): void;
}

export interface SessionLifecycle {
  ready: Promise<void>;
  attach(relay: LifecycleRelay): void;
  teardown(): Promise<void>;
}

export function installSessionLifecycle(
  chrome: ExtensionChrome,
  onError: (error: unknown) => void = error => console.error('Muniment lifecycle cleanup failed', error),
): SessionLifecycle {
  let relay: LifecycleRelay | undefined;
  let removeRelayListeners: Array<() => void> = [];
  let tearingDown: Promise<void> | undefined;
  const lifecycle = new AnchorLifecycle(chrome, () => { runTeardown(); });
  const ready = lifecycle.start();

  function report(promise: Promise<unknown>): void {
    void promise.catch(onError);
  }

  function runTeardown(): void {
    report(teardown());
  }

  function teardown(): Promise<void> {
    if (tearingDown)
      return tearingDown;
    const activeRelay = relay;
    relay = undefined;
    for (const remove of removeRelayListeners)
      remove();
    removeRelayListeners = [];
    // Relay revocation is synchronous even when MV3 discards asynchronous work.
    activeRelay?.close();
    tearingDown = lifecycle.teardown().finally(() => { tearingDown = undefined; });
    return tearingDown;
  }

  function attach(nextRelay: LifecycleRelay): void {
    if (relay) {
      nextRelay.close();
      return;
    }
    relay = nextRelay;
    removeRelayListeners = [
      nextRelay.onEvent(event => {
        if (event.method === 'muniment.connect')
          report(ready.then(() => relay === nextRelay ? lifecycle.connect() : undefined));
        else if (event.method === 'muniment.teardown')
          runTeardown();
      }),
      nextRelay.onClose(() => {
        if (relay === nextRelay)
          runTeardown();
      }),
    ];
  }

  chrome.runtime.onConnect.addListener(port => {
    if (port.name === ANCHOR_PORT)
      port.onDisconnect.addListener(runTeardown);
  });
  chrome.runtime.onSuspend.addListener(runTeardown);
  report(ready);
  return { ready, attach, teardown };
}
