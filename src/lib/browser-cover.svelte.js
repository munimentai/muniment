// The full-window dialogs open now. The native browser view draws above the
// page, so it hides while any of them is open and returns after the last closes.
let covers = $state(0)

export const browserCovered = () => covers > 0

// Holds the browser hidden until the returned release runs.
export function coverBrowser() {
  covers += 1
  let held = true
  return () => {
    if (!held) return
    held = false
    covers -= 1
  }
}
