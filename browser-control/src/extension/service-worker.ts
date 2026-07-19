import type { ExtensionChrome } from './anchor-lifecycle.js';
import { installSessionLifecycle } from './session-lifecycle.js';

declare const chrome: ExtensionChrome;

// The concrete loopback adapter attaches its attributed RelayConnection to
// this boundary; worker startup itself only performs fail-closed recovery.
export const sessionLifecycle = installSessionLifecycle(chrome);
