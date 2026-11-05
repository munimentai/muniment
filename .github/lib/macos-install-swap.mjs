// Swap a staged app bundle into its installed place. The installed bundle
// moves into a backup folder first, so a failed move restores it. A swap that
// succeeds removes the backup folder. The folder stays only when the restore
// also fails, and the error then names its path.
import { existsSync, mkdtempSync, renameSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

export function swapInstalledApp(stagedApp, installed, { backupRoot = tmpdir(), rename = renameSync } = {}) {
  const backup = mkdtempSync(join(backupRoot, "muniment-app-backup-"));
  const saved = join(backup, basename(installed));
  const hadApp = existsSync(installed);
  try {
    if (hadApp) rename(installed, saved);
    rename(stagedApp, installed);
  } catch (error) {
    if (hadApp && existsSync(saved)) {
      try { rename(saved, installed); }
      catch (restoreError) {
        throw new Error(`The new app could not be installed (${error.message}), and the old app could not be restored (${restoreError.message}). The old app is in ${backup}.`, { cause: error });
      }
    }
    rmSync(backup, { recursive: true, force: true });
    throw error;
  }
  rmSync(backup, { recursive: true, force: true });
}
