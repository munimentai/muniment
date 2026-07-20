export const onboardingLoadingState = { name: 'loading', homePath: '' }

export function onboardingStatusState(status) {
  return status.configured
    ? { name: 'complete', homePath: status.homePath }
    : { name: 'choosing', homePath: status.homePath }
}

export function onboardingPathState(state, homePath) {
  return { ...state, name: 'choosing', homePath, error: undefined }
}

export function onboardingConfirmingState(state) {
  return { ...state, name: 'confirming', error: undefined }
}

export function onboardingErrorState(state, error) {
  return {
    ...state,
    name: 'choosing',
    error: String(error || 'Muniment Home could not be created. Choose another folder and try again.'),
  }
}
