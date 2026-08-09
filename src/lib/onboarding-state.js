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

export function onboardingImportChoiceState(state) {
  return { ...state, name: 'import-choice', error: undefined }
}

export function onboardingFinalizingState(state) {
  return { name: 'finalizing', homePath: state.homePath, error: undefined }
}

export function onboardingConfirmedState(state, status) {
  return state.savedHomePath || state.name === 'finalizing'
    ? onboardingStatusState(status)
    : { name: 'import-choice', homePath: status.homePath }
}

export function onboardingPreviewingState(state, archivePath) {
  return { name: 'previewing', homePath: state.homePath, archivePath, manifest: undefined, selectedNames: [], extractedEntries: undefined, error: undefined }
}

export function onboardingPreviewState(state, manifest) {
  return { ...state, name: 'reviewing', manifest, selectedNames: [], extractedEntries: undefined, error: undefined }
}

export function onboardingSelectionState(state, entryName, selected) {
  const selectedNames = selected
    ? [...state.selectedNames, entryName]
    : state.selectedNames.filter((name) => name !== entryName)
  return { ...state, selectedNames, error: undefined }
}

export function onboardingExtractingState(state) {
  return { ...state, name: 'extracting', error: undefined }
}

export function onboardingExtractionState(state, extractedEntries) {
  return { ...state, name: 'approved-review', extractedEntries, error: undefined }
}

export function onboardingImportSavingState(state) {
  return { ...state, name: 'import-saving', error: undefined, errorKind: undefined, conflictPath: undefined }
}

export function onboardingImportSavedState(state) {
  return { name: 'complete', homePath: state.homePath }
}

export function onboardingImportErrorState(state, error) {
  if (error?.kind === 'invalidInput') {
    return {
      ...state,
      name: 'import-invalid',
      errorKind: 'invalidInput',
      error: 'The approved files are no longer valid. Return to archive review and review them again.',
    }
  }
  if (error?.kind === 'destinationConflict') {
    return {
      ...state,
      name: 'approved-review',
      errorKind: 'destinationConflict',
      conflictPath: typeof error.relativePath === 'string' && error.relativePath ? error.relativePath : undefined,
      error: 'That Home already contains an item at the proposed destination. Choose a different Home folder and try again.',
    }
  }
  if (error?.kind === 'secretRejected') {
    const sourceName = typeof error.sourceName === 'string' && error.sourceName ? error.sourceName : undefined
    return {
      ...state,
      name: 'approved-review',
      errorKind: 'secretRejected',
      conflictPath: undefined,
      error: sourceName
        ? `The approved file “${sourceName}” contains a credential, so Muniment imported nothing. Return to archive review and leave out that file.`
        : 'An approved file contains a credential, so Muniment imported nothing. Return to archive review and leave out the file that holds it.',
    }
  }
  return {
    ...state,
    name: 'approved-review',
    errorKind: 'saveFailed',
    conflictPath: undefined,
    error: 'The import could not be saved. Check that the Home folder is available and try again.',
  }
}

export function onboardingConfirmedHomePathState(state, homePath) {
  return { ...state, name: 'approved-review', homePath, error: undefined, errorKind: undefined, conflictPath: undefined }
}

export function onboardingReturnToArchiveReviewState(state) {
  return { ...state, name: 'reviewing', error: undefined }
}

export function onboardingExtractionErrorState(state, error) {
  const messages = {
    emptySelection: 'Select at least one file to continue.',
    invalidSelection: 'The selected files no longer match this archive. Review them and try again.',
    duplicateSelection: 'The file selection is invalid. Review it and try again.',
    directorySelection: 'A selected item is not a supported file. Review the selection and try again.',
    unknownSelection: 'A selected file is no longer present in this archive. Choose the ZIP again to review it.',
  }
  return {
    ...state,
    name: 'reviewing',
    error: messages[error?.kind] ?? 'The approved files could not be read. Your selection is unchanged; try again or choose another ZIP.',
  }
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

export function onboardingErrorState(state, error) {
  return {
    ...state,
    name: state.name === 'finalizing' ? 'import-choice' : state.savedHomePath ? 'settings' : 'choosing',
    error: String(error || 'Muniment Home could not be created. Choose another folder and try again.'),
  }
}
