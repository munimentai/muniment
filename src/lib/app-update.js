// The owner's install of a downloaded app update stops the replies this
// computer's runtime runs. They resume after the update, so the install
// names how many it stops and asks first.

// How many replies an install now stops, or 0 when the runtime does not say.
export async function interruptionCount(tauri) {
  try {
    const count = Number(await tauri.invoke('app_update_interruptions'))
    return Number.isInteger(count) && count > 0 ? count : 0
  } catch {
    return 0
  }
}

export function interruptionMessage(count) {
  return count === 1
    ? 'Installing now stops 1 running reply. It resumes after the update.'
    : `Installing now stops ${count} running replies. They resume after the update.`
}
