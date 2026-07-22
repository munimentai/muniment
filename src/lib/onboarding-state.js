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
  return { ...state, name: 'pre-triage', extractedEntries, error: undefined }
}

export function onboardingTriagingState(state) {
  return { ...state, name: 'triaging', report: undefined, error: undefined }
}

export function onboardingTriageReportState(state, response) {
  return { ...state, name: 'triage-review', report: response.report, error: undefined }
}

export function onboardingTriageErrorState(state, error) {
  const messages = {
    empty: 'No approved files are available. Return to archive review and select at least one file.',
    tooManyEntries: 'Too many approved files were selected. Return to archive review and choose fewer files.',
    malformedSource: 'An approved file has invalid source information. Return to archive review and choose the ZIP again.',
    inputTooLarge: 'The approved files are too large to review together. Return to archive review and choose fewer files.',
    localAiUnavailable: 'Local AI is unavailable. Start the local model and try again.',
    transportFailed: 'Local AI could not be reached. Check that the local model is running and try again.',
    invalidModelResponse: 'Local AI returned an incomplete proposal. Try generating it again.',
    requestFailed: 'The proposal could not be generated. Try again.',
  }
  return {
    ...state,
    name: 'triage-error',
    error: messages[error?.kind] ?? 'The proposal could not be generated. Try again.',
  }
}

export function onboardingTriageConfirmedState(state) {
  return { ...state, name: 'triage-confirmed', error: undefined }
}

export function onboardingReturnToArchiveReviewState(state) {
  return { ...state, name: 'reviewing', report: undefined, error: undefined }
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

export function onboardingCompleteState(state) {
  return { name: 'complete', homePath: state.homePath }
}

export function onboardingErrorState(state, error) {
  return {
    ...state,
    name: state.name === 'finalizing' ? 'import-choice' : state.savedHomePath ? 'settings' : 'choosing',
    error: String(error || 'Muniment Home could not be created. Choose another folder and try again.'),
  }
}
