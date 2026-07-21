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

export function onboardingConfirmedState(state, status) {
  return state.savedHomePath
    ? onboardingStatusState(status)
    : { name: 'import-choice', homePath: status.homePath }
}

export function onboardingPreviewingState(state, archivePath) {
  return { ...state, name: 'previewing', archivePath, manifest: undefined, error: undefined }
}

export function onboardingPreviewState(state, manifest) {
  return { ...state, name: 'reviewing', manifest, error: undefined }
}

export function onboardingPreviewErrorState(state, error) {
  const messages = {
    notFound: 'That archive could not be found. Choose a different ZIP file.',
    archiveTooLarge: 'That archive is too large to preview. Choose a smaller ZIP file.',
    invalidArchive: 'That file is not a readable ZIP archive. Choose a different export ZIP.',
    tooManyEntries: 'That archive contains too many files to preview. Choose a different export ZIP.',
    entryTooLarge: 'A file in that archive is too large to preview. Choose a different export ZIP.',
    totalTooLarge: 'That archive expands beyond the preview limit. Choose a smaller export ZIP.',
    pathTraversal: 'That archive contains an unsafe file path. Choose a different export ZIP.',
    pathTooDeep: 'That archive contains a file nested too deeply. Choose a different export ZIP.',
    duplicateEntry: 'That archive contains duplicate file names. Choose a different export ZIP.',
    encrypted: 'That archive contains an encrypted file. Choose an unencrypted export ZIP.',
    symlink: 'That archive contains a symbolic link. Choose a different export ZIP.',
    unsupportedEntry: 'That archive contains an unsupported file type. Choose a different export ZIP.',
    invalidText: 'That archive contains text Muniment cannot read. Choose a different export ZIP.',
    io: 'That archive could not be read. Check access or choose a different ZIP file.',
  }
  return {
    ...state,
    name: 'import-choice',
    manifest: undefined,
    error: messages[error?.kind] ?? 'That archive could not be previewed. Choose a different ZIP file.',
  }
}

export function onboardingCompleteState(state) {
  return { name: 'complete', homePath: state.homePath }
}

export function onboardingErrorState(state, error) {
  return {
    ...state,
    name: state.savedHomePath ? 'settings' : 'choosing',
    error: String(error || 'Muniment Home could not be created. Choose another folder and try again.'),
  }
}
