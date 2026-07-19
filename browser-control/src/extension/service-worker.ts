import type { ExtensionChrome } from './anchor-lifecycle.js';
import { authorizedRelayProvider, type AuthorizedRelayProvider } from './relay-provider.js';
import { LoopbackRelayAdapter } from './loopback-relay.js';
import { installSessionLifecycle } from './session-lifecycle.js';

declare const chrome: ExtensionChrome;

export function installServiceWorker(extensionChrome: ExtensionChrome, relayProvider: AuthorizedRelayProvider, relayAdapter?: LoopbackRelayAdapter) {
  const sessionLifecycle = installSessionLifecycle(extensionChrome, undefined, () => relayAdapter?.close());
  relayProvider.subscribe(relay => sessionLifecycle.attach(relay));
  extensionChrome.runtime.onSuspend.addListener(() => relayAdapter?.close());
  return sessionLifecycle;
}

// Endpoint discovery and pairing handoff are supplied by the loopback adapter;
// this provider is the extension-side composition point for its authorized relay.
export const loopbackRelayAdapter = new LoopbackRelayAdapter(authorizedRelayProvider);
export const sessionLifecycle = installServiceWorker(chrome, authorizedRelayProvider, loopbackRelayAdapter);
