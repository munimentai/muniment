import type { ExtensionChrome } from './anchor-lifecycle.js';
import { installSessionLifecycle } from './session-lifecycle.js';

declare const chrome: ExtensionChrome;

installSessionLifecycle(chrome);
