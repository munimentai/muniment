import { AnchorLifecycle, isLifecycleCommand, type ExtensionChrome } from './anchor-lifecycle.js';

declare const chrome: ExtensionChrome;

const lifecycle = new AnchorLifecycle(chrome);
const startup = lifecycle.start();
const anchorPorts = new Set<object>();

chrome.runtime.onConnect.addListener(port => {
  if (port.name !== 'muniment-anchor-lifecycle')
    return;
  anchorPorts.add(port);
  port.onDisconnect.addListener(() => anchorPorts.delete(port));
});

chrome.runtime.onMessage.addListener(message => {
  if (!isLifecycleCommand(message))
    return;
  return startup.then<unknown>(() => message.type === 'muniment.connect' ? lifecycle.connect() : lifecycle.teardown());
});

chrome.runtime.onSuspend.addListener(() => {
  void lifecycle.teardown();
});
