// Recreate the per-user layout that keeps its install-directory shortcut during upgrades.
export function legacyPerUserTemplate(template) {
  const cleanup = /\s*<RemoveFile Id="LegacyUninstallShortcut"[^>]*\/>/g;
  const shortcut = /(<Shortcut Id="UninstallShortcut"\s+)Directory="ApplicationProgramsFolder"/g;
  if ([...template.matchAll(cleanup)].length !== 1 || [...template.matchAll(shortcut)].length !== 1
      || !template.includes('<RemoveShortcuts>Installed AND NOT UPGRADINGPRODUCTCODE</RemoveShortcuts>')) {
    throw new Error('The legacy per-user fixture requires one shortcut, one cleanup rule, and the upgrade shortcut condition.');
  }
  return template.replace(cleanup, '').replace(shortcut, '$1Directory="INSTALLDIR"');
}
