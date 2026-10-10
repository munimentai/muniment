// Settings that live on this device only. Pages use them for choices whose
// runtime behavior has not shipped yet, so the choice is kept until it does.
export function readDevicePreference(key, fallback) {
  try {
    const stored = JSON.parse(localStorage.getItem(key))
    return stored && typeof stored === 'object' ? { ...fallback, ...stored } : { ...fallback }
  } catch (_) {
    return { ...fallback }
  }
}

export function saveDevicePreference(key, value) {
  try { localStorage.setItem(key, JSON.stringify(value)) } catch (_) {}
  return value
}
