$ErrorActionPreference = "Stop"

$bundleRoot = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle"
$nsis = Get-ChildItem (Join-Path $bundleRoot "nsis") -Filter "*-setup.exe" -File
$machineMsi = Get-ChildItem (Join-Path $bundleRoot "msi") -Filter "*-machine.msi" -File
if ($nsis.Count -ne 1 -or $machineMsi.Count -ne 1) {
  throw "Expected exactly one NSIS installer and one per-machine MSI"
}

# NSIS /S is case-sensitive. The default NSIS bundle is per-user.
$nsisProcess = Start-Process $nsis.FullName -ArgumentList "/S" -Wait -PassThru
if ($nsisProcess.ExitCode -ne 0) { throw "Silent NSIS install failed: $($nsisProcess.ExitCode)" }
$nsisUninstaller = Join-Path $env:LOCALAPPDATA "muniment\uninstall.exe"
if (-not (Test-Path $nsisUninstaller)) { throw "NSIS uninstaller not found at $nsisUninstaller" }
$nsisUninstall = Start-Process $nsisUninstaller -ArgumentList "/S" -Wait -PassThru
if ($nsisUninstall.ExitCode -ne 0) { throw "Silent NSIS uninstall failed: $($nsisUninstall.ExitCode)" }

$msiInstall = Start-Process msiexec.exe -ArgumentList "/i `"$($machineMsi.FullName)`" /qn /norestart" -Wait -PassThru
if ($msiInstall.ExitCode -ne 0) { throw "Silent MSI install failed: $($msiInstall.ExitCode)" }
$machineKey = "HKLM:\Software\Muniment\muniment"
if ((Get-ItemPropertyValue $machineKey InstallDir) -notlike "$env:ProgramFiles\*") {
  throw "Per-machine MSI did not register a Program Files install in HKLM"
}
$msiUninstall = Start-Process msiexec.exe -ArgumentList "/x `"$($machineMsi.FullName)`" /qn /norestart" -Wait -PassThru
if ($msiUninstall.ExitCode -ne 0) { throw "Silent MSI uninstall failed: $($msiUninstall.ExitCode)" }
if (Test-Path $machineKey) { throw "Machine registration remains after MSI uninstall" }

Write-Host "Windows silent installer verification OK"
