import { Channel, invoke } from '@tauri-apps/api/core'

// App commands release Tauri's plugin lock before waiting for the CEF UI thread.
export async function register(shortcut, onShortcut) {
  const handler = new Channel()
  handler.onmessage = onShortcut
  await invoke('shortcut_register', { shortcut, handler })
}

export async function unregister(shortcut) {
  await invoke('shortcut_unregister', { shortcut })
}
