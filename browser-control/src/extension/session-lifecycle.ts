import { AnchorLifecycle, isLifecycleCommand, type ChromePort, type ExtensionChrome } from './anchor-lifecycle.js';

export const DESKTOP_RELAY_PORT = 'muniment-desktop-relay';
export const ANCHOR_PORT = 'muniment-anchor-lifecycle';

export function installSessionLifecycle(chrome: ExtensionChrome): { ready: Promise<void>; teardown(): Promise<void> } {
  let relay: ChromePort | undefined;
  let tearingDown: Promise<void> | undefined;
  const lifecycle = new AnchorLifecycle(chrome, () => { void teardown(); });
  const ready = lifecycle.start();

  function teardown(): Promise<void> {
    if (tearingDown)
      return tearingDown;
    const activeRelay = relay;
    relay = undefined;
    // Disconnecting the transport is synchronous, so suspend cannot leave the
    // desktop session live while Chrome discards the cleanup promise.
    if (activeRelay) {
      try { activeRelay.disconnect(); } catch { /* The transport is already gone. */ }
    }
    tearingDown = lifecycle.teardown().finally(() => { tearingDown = undefined; });
    return tearingDown;
  }

  chrome.runtime.onConnect.addListener(port => {
    if (port.name === ANCHOR_PORT) {
      port.onDisconnect.addListener(() => { void teardown(); });
      return;
    }
    if (port.name !== DESKTOP_RELAY_PORT)
      return;

    if (relay && relay !== port) {
      try { port.disconnect(); } catch { /* Reject a second session transport. */ }
      return;
    }
    relay = port;
    port.onMessage.addListener(message => {
      if (!isLifecycleCommand(message))
        return;
      void ready.then(async () => {
        if (relay !== port)
          return;
        if (message.type === 'muniment.connect')
          await lifecycle.connect();
        else
          await teardown();
      });
    });
    port.onDisconnect.addListener(() => {
      if (relay === port)
        void teardown();
    });
  });

  chrome.runtime.onSuspend.addListener(() => { void teardown(); });
  return { ready, teardown };
}
