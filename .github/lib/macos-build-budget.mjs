// The nightly VM allows 80 minutes, including setup and asset upload.
export const MACOS_BUILD_SECONDS = 4800;
export const MACOS_UPLOAD_RESERVE_SECONDS = 600;

export const macosBuildDeadline = (value = String(MACOS_BUILD_SECONDS)) => {
  const seconds = Number(value);
  if (!/^-?\d+$/.test(value) || !Number.isSafeInteger(seconds) || seconds > MACOS_BUILD_SECONDS) {
    throw new Error(`MACOS_BUILD_REMAINING_SECONDS must be an integer at most ${MACOS_BUILD_SECONDS}`);
  }
  return performance.now() + (seconds - MACOS_UPLOAD_RESERVE_SECONDS) * 1000;
};

export const remainingBuildMilliseconds = (deadline) => {
  const remaining = deadline - performance.now();
  if (remaining <= 0) throw new Error("macOS build FAILED cause=build-timeout");
  return Math.ceil(remaining);
};
