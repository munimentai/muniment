export const firstRunErrors = {
  homeUnavailable: 'Home is unavailable. Retry or choose a folder.',
  homeConfirm: 'Muniment could not create Home. Check folder access and try Send again.',
  startup: 'Muniment could not finish startup. Choose a model again.',
  sessionStatus: 'Muniment could not read session status. Choose a model again.',
  localMode: 'Muniment could not enter local mode. Choose a model again.',
  runtime: 'The runtime is not connected yet. Choose a model again.',
}

export function firstRunError(tauri, cause) {
  const message = firstRunErrors[cause]
  console.error(message)
  // Send only a fixed cause. Drafts, paths, and credentials stay out of the log.
  void tauri.invoke('onboarding_model_settings_error', { cause }).catch(() => {
    console.error('Muniment could not write the first-run error to stderr.')
  })
  return new Error(message)
}
