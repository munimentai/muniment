$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "windows-msi-registration.ps1")
& (Join-Path $PSScriptRoot "windows-msi-registration.tests.ps1")

$bundleRoot = Join-Path $PSScriptRoot "..\src-tauri\target\release\bundle"
$nsis = @(Get-ChildItem (Join-Path $bundleRoot "nsis") -Filter "*-setup.exe" -File)
$machineMsi = @(Get-ChildItem (Join-Path $bundleRoot "msi") -Filter "*-machine.msi" -File)
$regularMsi = @(Get-ChildItem (Join-Path $bundleRoot "msi") -Filter "*.msi" -File |
  Where-Object { $_.Name -notlike "*-machine.msi" })
$upgradeBaseMsi = Join-Path $bundleRoot "machine-upgrade-base.msi"
$userUpgradeBaseMsi = Join-Path $bundleRoot "per-user-upgrade-base.msi"
if ($nsis.Count -ne 1 -or $machineMsi.Count -ne 1 -or $regularMsi.Count -ne 1 -or
    -not (Test-Path $upgradeBaseMsi) -or -not (Test-Path $userUpgradeBaseMsi)) {
  throw "Expected one NSIS installer, one regular MSI, one machine MSI, and both upgrade-base MSIs."
}

function Assert-CefInstallation([string]$Directory) {
  foreach ($name in @("muniment-desktop.exe", "muniment-desktop.dll", "libcef.dll", "chrome_elf.dll", "icudtl.dat", "v8_context_snapshot.bin", "resources.pak", "locales\en-US.pak", "CEF-CREDITS.html")) {
    $file = Join-Path $Directory $name
    if (-not (Test-Path -LiteralPath $file -PathType Leaf) -or (Get-Item -LiteralPath $file).Length -eq 0) {
      throw "The installed CEF resource is missing or empty: $file"
    }
  }
}

function Assert-MsiPayload($Package, $Directory) {
  $expanded = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
  New-Item -ItemType Directory -Path $expanded | Out-Null
  try {
    $resolvedPackage = (Resolve-Path -LiteralPath $Package).Path
    $process = Start-Process msiexec.exe -ArgumentList "/a `"$resolvedPackage`" /qn /norestart TARGETDIR=`"$expanded`"" -Wait -PassThru
    if ($process.ExitCode -notin @(0, 3010)) { throw "The MSI admin extraction failed: $($process.ExitCode)." }
    & node (Join-Path $PSScriptRoot "windows-msi-payload.mjs") $expanded $Directory
    if ($LASTEXITCODE -ne 0) { throw "The installed payload differs from the MSI admin image." }
  } finally {
    Remove-Item -LiteralPath $expanded -Recurse -Force
  }
}

function Write-MsiProperties($Package) {
  $installer = $database = $summary = $null
  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase((Resolve-Path -LiteralPath $Package).Path, 0)
    $values = @{}
    foreach ($name in @("ALLUSERS", "MSIINSTALLPERUSER")) {
      $view = $record = $null
      try {
        $view = $database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property`` = '$name'")
        $view.Execute()
        $record = $view.Fetch()
        $values[$name] = if ($null -eq $record) { "absent" } else { $record.StringData(1) }
      } finally {
        if ($null -ne $record) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($record) }
        if ($null -ne $view) {
          $view.Close()
          [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($view)
        }
      }
    }
    $summary = $database.SummaryInformation(0)
    # WiX marks perUser packages with bit 3 of PID_WORDCOUNT (15).
    # This reports the package marker, not the resolved install context.
    $scope = if (([int]$summary.Property(15) -band 8) -ne 0) { "perUser" } else { "perMachine" }
    Write-Host "msi properties $(Split-Path -Leaf $Package): ALLUSERS=$($values.ALLUSERS) MSIINSTALLPERUSER=$($values.MSIINSTALLPERUSER) InstallScope=$scope"
  } finally {
    foreach ($comObject in @($summary, $database, $installer)) {
      if ($null -ne $comObject) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($comObject) }
    }
  }
}

function Write-MsiScopeLog($LogPath) {
  if (-not (Test-Path -LiteralPath $LogPath)) {
    Write-Host "The per-user MSI verbose log is absent."
    return
  }
  foreach ($name in @("ALLUSERS", "MSIINSTALLPERUSER", "UserSID", "LogonUser", "MsiRunningElevated")) {
    $lines = @(Select-String -LiteralPath $LogPath -Pattern "PROPERTY CHANGE: (Adding|Modifying|Deleting) $name property\b|Property\([CS]\): $name =")
    if ($lines.Count -eq 0) { Write-Host "The per-user MSI log has no $name lines." }
    $lines | ForEach-Object { Write-Host $_.Line }
  }
}

function Get-UserRegistrations($Hive) {
  foreach ($suffix in @("Software\Microsoft\Windows\CurrentVersion\Uninstall", "Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall")) {
    $root = "$Hive\$suffix"
    if (Test-Path -LiteralPath $root) {
      Get-ChildItem -LiteralPath $root | Where-Object { (Get-ItemProperty -LiteralPath $_.PSPath).DisplayName -eq "muniment" }
    }
  }
}

function Invoke-Msi($Action, $Package, $Description, $LogPath) {
  $resolvedPackage = (Resolve-Path -LiteralPath $Package).Path
  $arguments = "$Action `"$resolvedPackage`" /qn /norestart"
  if ($LogPath) { $arguments += " /L*V `"$LogPath`"" }
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

foreach ($package in @($regularMsi[0].FullName, $machineMsi[0].FullName, $upgradeBaseMsi, $userUpgradeBaseMsi)) {
  Write-MsiProperties $package
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
Assert-CefInstallation (Split-Path $machineRuntime)
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
$userUninstallShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) "muniment\Uninstall muniment.lnk"
$userProductCode = Get-MsiProductCode $regularMsi[0].FullName
$sessionSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$userMsiLog = [IO.Path]::GetTempFileName()
try {
  try {
    Invoke-Msi "/i" $regularMsi[0].FullName "Silent regular MSI install" $userMsiLog
  } finally {
    Write-MsiScopeLog $userMsiLog
    $hkcu = @(Get-UserRegistrations "HKCU:")
    $hku = @(Get-UserRegistrations "Registry::HKEY_USERS\$sessionSid")
    $hklm = @(Get-MunimentRegistrations)
    Write-Host "per-user MSI registration: session SID=$sessionSid HKU\$sessionSid=$($hku.Count) hkcu=$($hkcu.Count) hklm=$($hklm.Count)"
    $userRegistrations = @(Get-MsiRegistrations $regularMsi[0].FullName $sessionSid)
    Write-Host "The regular MSI has $($userRegistrations.Count) Windows Installer registrations."
    foreach ($registration in $userRegistrations) {
      Write-Host "The MSI registration has ProductCode=$($registration.ProductCode) context=$($registration.Context) SID=$($registration.UserSid) State=$($registration.State)."
    }
    $registryRegistration = Get-PerUserMsiRegistration $userProductCode $sessionSid
    Write-PerUserMsiRegistration $registryRegistration "installed"
  }
} finally {
  Remove-Item -LiteralPath $userMsiLog -Force
}
$userKey = "HKCU:\Software\Muniment\muniment"
try {
  # Windows Installer can store a per-user product's uninstall entry under HKLM.
  # Check its registered context, not the uninstall entry's hive: https://github.com/wixtoolset/issues/issues/9323
  Assert-MsiProductContext $userRegistrations $sessionSid
  Assert-PerUserMsiRegistration $registryRegistration $env:LOCALAPPDATA
  if (($hkcu.Count + $hklm.Count) -ne 1) {
    throw "The regular MSI must have exactly one uninstall registration."
  }
  if ((Get-ItemPropertyValue $userKey InstallDir).TrimEnd('\') -ne (Split-Path $userRuntime)) {
    throw "The per-user MSI must register its LocalAppData install path under HKCU."
  }
  if (Test-Path $machineKey) { throw "The per-user MSI wrote application registration under HKLM." }
  if (-not (Test-Path $userRuntime)) { throw "Regular MSI runtime not found at $userRuntime" }
  Assert-CefInstallation (Split-Path $userRuntime)
  if (Test-Path (Join-Path (Split-Path $userRuntime) "Uninstall muniment.lnk")) {
    throw "The per-user MSI must keep the uninstall shortcut outside the install directory."
  }
  if (-not (Test-Path -LiteralPath $userUninstallShortcut -PathType Leaf)) {
    throw "The per-user MSI must create the uninstall shortcut in the Start menu."
  }
  Assert-MsiPayload $regularMsi[0].FullName (Split-Path $userRuntime)
} finally {
  try {
    Invoke-Msi "/x" $regularMsi[0].FullName "Silent regular MSI uninstall"
  } finally {
    $registryRegistration = Get-PerUserMsiRegistration $userProductCode $sessionSid
    Write-PerUserMsiRegistration $registryRegistration "uninstalled"
    Assert-PerUserMsiRegistration $registryRegistration $env:LOCALAPPDATA -Absent
    if (@(Get-MsiRegistrations $regularMsi[0].FullName $sessionSid).Count -ne 0) {
      throw "The MSI product registration remains after the per-user uninstall."
    }
    if (Test-Path $userUninstallShortcut) { throw "The uninstall shortcut remains after the per-user MSI uninstall." }
    if (Test-Path $userRuntime) { throw "The runtime remains after the per-user MSI uninstall." }
    if (Test-Path $userKey) { throw "The application registration remains after the per-user MSI uninstall." }
  }
}

# Exercise the old MSI's upgrade condition, not a manually planted shortcut.
$legacyShortcut = Join-Path (Split-Path $userRuntime) "Uninstall muniment.lnk"
$oldUserProductCode = Get-MsiProductCode $userUpgradeBaseMsi
if ($oldUserProductCode -eq $userProductCode) { throw "The per-user upgrade requires distinct ProductCodes." }
try {
  Invoke-Msi "/i" $userUpgradeBaseMsi "Silent legacy per-user MSI install"
  Assert-MsiProductContext @(Get-MsiRegistrations $userUpgradeBaseMsi $sessionSid) $sessionSid
  if (-not (Test-Path -LiteralPath $legacyShortcut -PathType Leaf)) {
    throw "The legacy per-user MSI must create its shortcut in the install directory."
  }
  if (Test-Path -LiteralPath $userUninstallShortcut) {
    throw "The legacy per-user MSI must not create the new Start menu uninstall shortcut."
  }

  Invoke-Msi "/i" $regularMsi[0].FullName "Silent per-user MSI in-place upgrade"
  if (@(Get-MsiRegistrations $userUpgradeBaseMsi $sessionSid).Count -ne 0) {
    throw "The legacy per-user MSI remains registered after the upgrade."
  }
  Assert-MsiProductContext @(Get-MsiRegistrations $regularMsi[0].FullName $sessionSid) $sessionSid
  if (Test-Path -LiteralPath $legacyShortcut) { throw "The legacy uninstall shortcut remains after the upgrade." }
  if (-not (Test-Path -LiteralPath $userUninstallShortcut -PathType Leaf)) {
    throw "The upgraded per-user MSI must create the Start menu uninstall shortcut."
  }
  Assert-MsiPayload $regularMsi[0].FullName (Split-Path $userRuntime)
} finally {
  foreach ($package in @($regularMsi[0].FullName, $userUpgradeBaseMsi)) {
    if (@(Get-MsiRegistrations $package $sessionSid).Count -ne 0) {
      Invoke-Msi "/x" $package "Silent per-user upgrade cleanup"
    }
  }
  Remove-Item -LiteralPath $userUpgradeBaseMsi -Force
}
foreach ($shortcut in @($legacyShortcut, $userUninstallShortcut)) {
  if (Test-Path -LiteralPath $shortcut) { throw "An uninstall shortcut remains after the upgrade uninstall." }
}
if (Test-Path -LiteralPath $userRuntime) { throw "The runtime remains after the upgrade uninstall." }
if (Test-Path $userKey) { throw "The application registration remains after the upgrade uninstall." }

# NSIS /S is case-sensitive. Its silent uninstall retains the default install path under the shared HKCU key.
# Run NSIS after both MSI lifecycles so the retained key cannot affect MSI assertions.
$nsisProcess = Start-Process $nsis.FullName -ArgumentList "/S" -Wait -PassThru
if ($nsisProcess.ExitCode -ne 0) { throw "Silent NSIS install failed: $($nsisProcess.ExitCode)" }
if (-not (Test-Path $userRuntime)) { throw "NSIS runtime not found at $userRuntime" }
Assert-CefInstallation (Split-Path $userRuntime)
$nsisUninstaller = Join-Path $env:LOCALAPPDATA "muniment\uninstall.exe"
if (-not (Test-Path $nsisUninstaller)) { throw "NSIS uninstaller not found at $nsisUninstaller" }
$nsisUninstall = Start-Process $nsisUninstaller -ArgumentList "/S" -Wait -PassThru
if ($nsisUninstall.ExitCode -ne 0) { throw "Silent NSIS uninstall failed: $($nsisUninstall.ExitCode)" }
if (Test-Path $userRuntime) { throw "NSIS runtime remains after uninstall at $userRuntime" }

Write-Host "Windows silent installer verification OK"
