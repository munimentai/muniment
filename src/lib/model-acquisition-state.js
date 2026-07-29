export const requiredModelLoadingState = {
  status: { state: 'notInstalled' },
  downloadedBytes: 0,
  totalBytes: 0,
  folderSetupAvailable: true,
  aiFeaturesAvailable: false,
  retryingInBackground: false,
}

export function requiredModelPollActive(model) {
  return model.status?.state === 'installing' || model.retryingInBackground === true
}

export function requiredModelProgress(model) {
  const total = Number.isFinite(model.totalBytes) ? Math.max(0, model.totalBytes) : 0
  const downloaded = Number.isFinite(model.downloadedBytes) ? Math.max(0, model.downloadedBytes) : 0
  return { downloaded: Math.min(downloaded, total), total }
}

export function requiredModelLabel(model) {
  if (model.aiFeaturesAvailable) return 'Ready'
  if (model.status?.state === 'installing') return 'Downloading'
  if (model.retryingInBackground) return 'Retrying in background'
  if (model.status?.state === 'failed' || model.status?.state === 'cancelled') return 'Setup unavailable'
  return 'Starting setup'
}

export function requiredModelMessage(model) {
  if (model.aiFeaturesAvailable) return 'Local proposal generation is ready.'
  if (model.retryingInBackground) {
    return 'AI-dependent features are unavailable while the download retries. Folder setup and the rest of the app remain usable.'
  }
  if (model.status?.state === 'failed' || model.status?.state === 'cancelled') {
    return 'AI-dependent features are unavailable. Folder setup and the rest of the app remain usable.'
  }
  return 'AI-dependent features are unavailable while the model downloads. Folder setup and the rest of the app remain usable.'
}
