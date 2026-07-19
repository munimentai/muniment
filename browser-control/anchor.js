const CONNECTED_COPY = 'Muniment browser control is connected. Keep this tab open; closing it disconnects browser control.';
const DISCONNECTED_COPY = 'Browser control disconnected. Reconnect from the Muniment desktop app.';

// The URL state survives service-worker suspension and extension reload. A newly
// loaded page defaults to disconnected unless the current worker explicitly
// created it as this session's anchor.
const status = document.querySelector('#status');
const stateLabel = document.querySelector('#state-label');
const connected = location.hash === '#muniment-owned-connected';
function render(isConnected) {
  document.body.dataset.state = isConnected ? 'connected' : 'disconnected';
  if (stateLabel)
    stateLabel.textContent = isConnected ? 'Connected' : 'Disconnected';
  if (status)
    status.textContent = isConnected ? CONNECTED_COPY : DISCONNECTED_COPY;
}
render(connected);

if (connected) {
  const worker = chrome.runtime.connect({ name: 'muniment-anchor-lifecycle' });
  worker.onDisconnect.addListener(() => {
    history.replaceState(null, '', '#muniment-owned-disconnected');
    render(false);
  });
}
