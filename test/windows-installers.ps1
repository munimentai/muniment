$ErrorActionPreference = "Stop"

$bundleRoot = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle"
$nsis = Get-ChildItem (Join-Path $bundleRoot "nsis") -Filter "*-setup.exe" -File
$machineMsi = Get-ChildItem (Join-Path $bundleRoot "msi") -Filter "*-machine.msi" -File
$upgradeBaseMsi = Join-Path $bundleRoot ".machine-upgrade-base.msi"
if ($nsis.Count -ne 1 -or $machineMsi.Count -ne 1 -or -not (Test-Path $upgradeBaseMsi)) {
  throw "Expected exactly one NSIS installer, one per-machine MSI, and an upgrade-base MSI"
}

function Get-MsiProperty($Path, $Name) {
  $installer = New-Object -ComObject WindowsInstaller.Installer
  # Windows Installer exposes OpenDatabase through IDispatch. Be explicit about
  # the argument types: PowerShell's reflection binder otherwise passes the
  # persist mode as a generic PSObject on Windows PowerShell 5.1, which COM
  # rejects with DISP_E_TYPEMISMATCH on the desktop-CI image.
  [object[]]$openArguments = @([string](Resolve-Path $Path).Path, [int]0)
  $database = $installer.GetType().InvokeMember(
    "OpenDatabase",
    [System.Reflection.BindingFlags]::InvokeMethod,
    $null,
    $installer,
    $openArguments
  )
  $view = $database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='$Name'")
  $view.Execute()
  return $view.Fetch().StringData(1)
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

$oldProductCode = Get-MsiProperty $upgradeBaseMsi "ProductCode"
$newProductCode = Get-MsiProperty $machineMsi.FullName "ProductCode"
$oldUpgradeCode = Get-MsiProperty $upgradeBaseMsi "UpgradeCode"
$newUpgradeCode = Get-MsiProperty $machineMsi.FullName "UpgradeCode"
$expectedUpgradeCode = "C75B4A56-7D8B-5B99-9FC7-61EF0AABE84B"
if ($oldProductCode -eq $newProductCode) {
  throw "Upgrade fixture and release MSI must be distinct builds with different ProductCodes"
}
if ($oldUpgradeCode.Trim("{}").ToUpperInvariant() -ne $expectedUpgradeCode -or
    $newUpgradeCode.Trim("{}").ToUpperInvariant() -ne $expectedUpgradeCode) {
  throw "Machine MSI UpgradeCode is not stable: $oldUpgradeCode / $newUpgradeCode"
}

Invoke-Msi "/i `"$upgradeBaseMsi`" /qn /norestart" "Silent base MSI install"
Invoke-Msi "/i `"$($machineMsi.FullName)`" /qn /norestart" "Silent MSI in-place upgrade"

$machineKey = "HKLM:\Software\Muniment\muniment"
if ((Get-ItemPropertyValue $machineKey InstallDir) -notlike "$env:ProgramFiles\*") {
  throw "Per-machine MSI did not register a Program Files install in HKLM"
}
if (Test-Path "HKCU:\Software\Muniment\muniment") {
  throw "Per-machine MSI wrote application registration under HKCU"
}
$uninstallRoots = @(
  "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
  "HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
)
$newRegistration = $uninstallRoots | ForEach-Object { Join-Path $_ $newProductCode } | Where-Object { Test-Path $_ }
$oldRegistration = $uninstallRoots | ForEach-Object { Join-Path $_ $oldProductCode } | Where-Object { Test-Path $_ }
if ($newRegistration.Count -ne 1) { throw "Upgraded MSI is not registered exactly once under HKLM uninstall registration" }
if ($oldRegistration.Count -ne 0) { throw "Older MSI product remains registered after in-place upgrade" }

Invoke-Msi "/x `"$($machineMsi.FullName)`" /qn /norestart" "Silent MSI uninstall"
if (Test-Path $machineKey) { throw "Machine registration remains after MSI uninstall" }
if ($uninstallRoots | ForEach-Object { Join-Path $_ $newProductCode } | Where-Object { Test-Path $_ }) {
  throw "Machine uninstall registration remains after MSI uninstall"
}
Remove-Item $upgradeBaseMsi -Force

Write-Host "Windows silent installer verification OK"
