import type { ExtensionChrome } from './anchor-lifecycle.js';
import { authorizedRelayProvider, type AuthorizedRelayProvider } from './relay-provider.js';
import { installSessionLifecycle } from './session-lifecycle.js';

declare const chrome: ExtensionChrome;

export function installServiceWorker(extensionChrome: ExtensionChrome, relayProvider: AuthorizedRelayProvider) {
  const sessionLifecycle = installSessionLifecycle(extensionChrome);
  relayProvider.subscribe(relay => sessionLifecycle.attach(relay));
  return sessionLifecycle;
}

// Endpoint discovery and pairing handoff are supplied by the loopback adapter;
// this provider is the extension-side composition point for its authorized relay.
export const sessionLifecycle = installServiceWorker(chrome, authorizedRelayProvider);
