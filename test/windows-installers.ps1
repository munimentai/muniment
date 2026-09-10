$ErrorActionPreference = "Stop"

$bundleRoot = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle"
$nsis = @(Get-ChildItem (Join-Path $bundleRoot "nsis") -Filter "*-setup.exe" -File)
$machineMsi = @(Get-ChildItem (Join-Path $bundleRoot "msi") -Filter "*-machine.msi" -File)
$regularMsi = @(Get-ChildItem (Join-Path $bundleRoot "msi") -Filter "*.msi" -File |
  Where-Object { $_.Name -notlike "*-machine.msi" })
$upgradeBaseMsi = Join-Path $bundleRoot "machine-upgrade-base.msi"
if ($nsis.Count -ne 1 -or $machineMsi.Count -ne 1 -or $regularMsi.Count -ne 1 -or
    -not (Test-Path $upgradeBaseMsi)) {
  throw "Expected one NSIS installer, one regular MSI, one machine MSI, and one upgrade-base MSI"
}

function Invoke-Msi($Action, $Package, $Description) {
  $resolvedPackage = (Resolve-Path -LiteralPath $Package).Path
  $arguments = "$Action `"$resolvedPackage`" /qn /norestart"
  $process = Start-Process msiexec.exe -ArgumentList $arguments -Wait -PassThru
  if ($process.ExitCode -notin @(0, 3010)) { throw "$Description failed: $($process.ExitCode)" }
}

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

Invoke-Msi "/i" $upgradeBaseMsi "Silent base MSI install"
$baseRegistration = Get-MunimentRegistrations
if ($baseRegistration.Count -ne 1) { throw "Base MSI is not registered exactly once under HKLM uninstall registration" }
$oldProductCode = $baseRegistration[0].PSChildName

Invoke-Msi "/i" $machineMsi.FullName "Silent MSI in-place upgrade"
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
$machineRuntime = Join-Path $env:ProgramFiles "muniment\muniment-runtime.exe"
if (-not (Test-Path $machineRuntime)) {
  throw "Machine MSI runtime not found at $machineRuntime"
}
if (Test-Path "HKCU:\Software\Muniment\muniment") {
  throw "Per-machine MSI wrote application registration under HKCU"
}

Invoke-Msi "/x" $machineMsi.FullName "Silent MSI uninstall"
if (Test-Path $machineKey) { throw "Machine registration remains after MSI uninstall" }
if ((Get-MunimentRegistrations).Count -ne 0) {
  throw "Machine uninstall registration remains after MSI uninstall"
}
Remove-Item $upgradeBaseMsi -Force

$userRuntime = Join-Path $env:LOCALAPPDATA "muniment\muniment-runtime.exe"
$userKey = "HKCU:\Software\Muniment\muniment"
$userUninstallRoots = @(
  "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
  "HKCU:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
)
function Assert-UserMsiRegistrations($ExpectedUserCount, $Stage) {
  $userRegistrations = @($userUninstallRoots | ForEach-Object {
    if (Test-Path $_) {
      Get-ChildItem $_ | Where-Object { (Get-ItemProperty $_.PSPath).DisplayName -eq "muniment" }
    }
  })
  $machineRegistrations = @(Get-MunimentRegistrations)
  Write-Host "Per-user MSI ${Stage}: hkcu=$($userRegistrations.Count) hklm=$($machineRegistrations.Count)"
  if ($userRegistrations.Count -ne $ExpectedUserCount -or $machineRegistrations.Count -ne 0) {
    throw "Per-user MSI ${Stage}: expected hkcu=$ExpectedUserCount hklm=0"
  }
}

Assert-UserMsiRegistrations 0 "before install"
if ((Test-Path $userKey) -or (Test-Path $machineKey)) {
  throw "Application registration remains before the per-user MSI install"
}
try {
  Invoke-Msi "/i" $regularMsi[0].FullName "Silent regular MSI install"
  Assert-UserMsiRegistrations 1 "after install"
  $userInstallDir = Get-ItemPropertyValue $userKey InstallDir
  $expectedUserInstallDir = Join-Path $env:LOCALAPPDATA "muniment"
  if ($userInstallDir.TrimEnd('\') -ne $expectedUserInstallDir -or
      $userInstallDir -notlike "$env:USERPROFILE\*") {
    throw "Per-user MSI did not register an install directory under the user profile: $userInstallDir"
  }
  if (Test-Path $machineKey) { throw "Per-user MSI wrote application registration under HKLM" }
  if (-not (Test-Path $userRuntime)) { throw "Regular MSI runtime not found at $userRuntime" }
} finally {
  Invoke-Msi "/x" $regularMsi[0].FullName "Silent regular MSI uninstall"
}
Assert-UserMsiRegistrations 0 "after uninstall"
if ((Test-Path $userKey) -or (Test-Path $machineKey)) {
  throw "Application registration remains after the per-user MSI uninstall"
}
if (Test-Path $userRuntime) { throw "Per-user MSI runtime remains after uninstall at $userRuntime" }

# NSIS keeps its HKCU install-path key after silent uninstall.
# Run NSIS after both MSI checks so that key cannot affect their clean-state assertions.
# NSIS /S is case-sensitive.
$nsisProcess = Start-Process $nsis.FullName -ArgumentList "/S" -Wait -PassThru
if ($nsisProcess.ExitCode -ne 0) { throw "Silent NSIS install failed: $($nsisProcess.ExitCode)" }
if (-not (Test-Path $userRuntime)) { throw "NSIS runtime not found at $userRuntime" }
$nsisUninstaller = Join-Path $env:LOCALAPPDATA "muniment\uninstall.exe"
if (-not (Test-Path $nsisUninstaller)) { throw "NSIS uninstaller not found at $nsisUninstaller" }
$nsisUninstall = Start-Process $nsisUninstaller -ArgumentList "/S" -Wait -PassThru
if ($nsisUninstall.ExitCode -ne 0) { throw "Silent NSIS uninstall failed: $($nsisUninstall.ExitCode)" }
if (Test-Path $userRuntime) { throw "NSIS runtime remains after uninstall at $userRuntime" }

Write-Host "Windows silent installer verification OK"
