export const onboardingLoadingState = { name: 'loading', homePath: '' }

export function onboardingStatusState(status) {
  return status.configured
    ? { name: 'complete', homePath: status.homePath }
    : { name: 'choosing', homePath: status.homePath }
}

export function onboardingPathState(state, homePath) {
  return { ...state, name: state.savedHomePath ? 'settings' : 'choosing', homePath, error: undefined }
}

export function onboardingSettingsState(state) {
  return { name: 'settings', homePath: state.homePath, savedHomePath: state.homePath }
}

export function onboardingCancelSettingsState(state) {
  return { name: 'complete', homePath: state.savedHomePath }
}

export function onboardingConfirmingState(state) {
  return { ...state, name: state.savedHomePath ? 'confirming-settings' : 'confirming', error: undefined }
}

export function onboardingErrorState(state, error) {
  return {
    ...state,
    name: state.savedHomePath ? 'settings' : 'choosing',
    error: String(error || 'Muniment Home could not be created. Choose another folder and try again.'),
  }
}
