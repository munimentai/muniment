$ErrorActionPreference = "Stop"

$bundleRoot = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle"
$nsis = Get-ChildItem (Join-Path $bundleRoot "nsis") -Filter "*-setup.exe" -File
$machineMsi = Get-ChildItem (Join-Path $bundleRoot "msi") -Filter "*-machine.msi" -File
$upgradeBaseMsi = Join-Path $bundleRoot ".machine-upgrade-base.msi"
if ($nsis.Count -ne 1 -or $machineMsi.Count -ne 1 -or -not (Test-Path $upgradeBaseMsi)) {
  throw "Expected exactly one NSIS installer, one per-machine MSI, and an upgrade-base MSI"
}

function Invoke-Msi($Arguments, $Description) {
  $process = Start-Process msiexec.exe -ArgumentList $Arguments -Wait -PassThru
  if ($process.ExitCode -notin @(0, 3010)) { throw "$Description failed: $($process.ExitCode)" }
}

# NSIS /S is case-sensitive. The default NSIS bundle is per-user.
$nsisProcess = Start-Process $nsis.FullName -ArgumentList "/S" -Wait -PassThru
if ($nsisProcess.ExitCode -ne 0) { throw "Silent NSIS install failed: $($nsisProcess.ExitCode)" }
$nsisUninstaller = Join-Path $env:LOCALAPPDATA "muniment\uninstall.exe"
if (-not (Test-Path $nsisUninstaller)) { throw "NSIS uninstaller not found at $nsisUninstaller" }
$nsisUninstall = Start-Process $nsisUninstaller -ArgumentList "/S" -Wait -PassThru
if ($nsisUninstall.ExitCode -ne 0) { throw "Silent NSIS uninstall failed: $($nsisUninstall.ExitCode)" }

$machineKey = "HKLM:\Software\Muniment\muniment"
$uninstallRoots = @(
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
  "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
)
function Get-MunimentRegistrations {
  return @($uninstallRoots | ForEach-Object {
    Get-ChildItem $_ | Where-Object { (Get-ItemProperty $_.PSPath).DisplayName -eq "muniment" }
  })
}

Invoke-Msi "/i `"$upgradeBaseMsi`" /qn /norestart" "Silent base MSI install"
$baseRegistration = Get-MunimentRegistrations
if ($baseRegistration.Count -ne 1) { throw "Base MSI is not registered exactly once under HKLM uninstall registration" }
$oldProductCode = $baseRegistration[0].PSChildName

Invoke-Msi "/i `"$($machineMsi.FullName)`" /qn /norestart" "Silent MSI in-place upgrade"
$newRegistration = Get-MunimentRegistrations
if ($newRegistration.Count -ne 1) { throw "Upgraded MSI is not registered exactly once under HKLM uninstall registration" }
$newProductCode = $newRegistration[0].PSChildName
if ($oldProductCode -eq $newProductCode) {
  throw "Upgrade fixture and release MSI did not produce distinct ProductCodes"
}
if ($uninstallRoots | ForEach-Object { Join-Path $_ $oldProductCode } | Where-Object { Test-Path $_ }) {
  throw "Older MSI product remains registered after in-place upgrade"
}
if ((Get-ItemPropertyValue $machineKey InstallDir) -notlike "$env:ProgramFiles\*") {
  throw "Per-machine MSI did not register a Program Files install in HKLM"
}
if (Test-Path "HKCU:\Software\Muniment\muniment") {
  throw "Per-machine MSI wrote application registration under HKCU"
}

Invoke-Msi "/x `"$($machineMsi.FullName)`" /qn /norestart" "Silent MSI uninstall"
if (Test-Path $machineKey) { throw "Machine registration remains after MSI uninstall" }
if ((Get-MunimentRegistrations).Count -ne 0) {
  throw "Machine uninstall registration remains after MSI uninstall"
}
Remove-Item $upgradeBaseMsi -Force

Write-Host "Windows silent installer verification OK"
