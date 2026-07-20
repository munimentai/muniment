const MODEL_NAME = 'Qwen3.5-4B'

export function formatModelBytes(bytes) {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / (1024 ** unit)
  return `${value.toFixed(unit === 0 || value >= 10 ? 0 : 1)} ${units[unit]}`
}

export function modelAcquisitionState(response) {
  const reportedDownloadedBytes = Math.max(0, Number(response?.downloadedBytes) || 0)
  const totalBytes = Math.max(0, Number(response?.totalBytes) || 0)
  const downloadedBytes = totalBytes > 0 ? Math.min(reportedDownloadedBytes, totalBytes) : reportedDownloadedBytes
  const state = response?.status?.state
  const active = state === 'installing' || response?.retryingInBackground === true
  const ready = response?.aiFeaturesAvailable === true
  const percent = totalBytes > 0
    ? Math.round((downloadedBytes / totalBytes) * 100)
    : 0

  return {
    modelName: MODEL_NAME,
    state,
    installing: state === 'installing',
    active: active && !ready,
    ready,
    failed: state === 'failed',
    retrying: response?.retryingInBackground === true,
    downloadedBytes,
    totalBytes,
    percent,
    folderSetupAvailable: response?.folderSetupAvailable !== false,
  }
}
