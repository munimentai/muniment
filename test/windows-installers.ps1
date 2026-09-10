$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "windows-msi-registration.ps1")

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

function Get-MsiRegistrations($Package) {
  $installer = $database = $view = $record = $products = $null
  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase((Resolve-Path -LiteralPath $Package).Path, 0)
    $view = $database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property`` = 'ProductCode'")
    [void]$view.Execute()
    $record = $view.Fetch()
    if ($null -eq $record) { throw "The MSI has no ProductCode." }
    $productCode = $record.StringData(1)
    # Query the current user's contexts and the machine context, not the uninstall registry hive.
    # https://learn.microsoft.com/en-us/windows/win32/msi/installer-productsex
    $products = $installer.ProductsEx($productCode, "", 7)
    foreach ($product in $products) {
      try {
        [PSCustomObject]@{
          ProductCode = $productCode
          Context = [int]$product.GetType().InvokeMember('Context', [Reflection.BindingFlags]::GetProperty, $null, $product, $null)
          UserSid = [string]$product.GetType().InvokeMember('UserSid', [Reflection.BindingFlags]::GetProperty, $null, $product, $null)
        }
      } finally {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($product)
      }
    }
  } finally {
    if ($null -ne $view) { [void]$view.Close() }
    foreach ($comObject in @($products, $record, $view, $database, $installer)) {
      if ($null -ne $comObject) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($comObject) }
    }
  }
}

function Assert-UserMsiRegistration($Registrations, $SessionSid) {
  # Context 1 means per-user managed. Context 2 means per-user unmanaged. Context 4 means per-machine.
  if ($Registrations.Count -ne 1 -or $Registrations[0].Context -notin @(1, 2) -or
      $Registrations[0].UserSid -ne $SessionSid) {
    throw "The regular MSI must register once for the current user, not the machine."
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

foreach ($package in @($regularMsi[0].FullName, $machineMsi[0].FullName, $upgradeBaseMsi)) {
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
if (Test-Path "HKCU:\Software\Muniment\muniment") {
  throw "Per-machine MSI wrote application registration under HKCU"
}

Invoke-Msi "/x" $machineMsi.FullName "Silent MSI uninstall"
if (Test-Path $machineKey) { throw "Machine registration remains after MSI uninstall" }
if ((Get-MunimentRegistrations).Count -ne 0) {
  throw "Machine uninstall registration remains after MSI uninstall"
}
Remove-Item $upgradeBaseMsi -Force

# NSIS /S is case-sensitive. Run the per-user installer after the machine-scope
# assertions so its expected HKCU registration cannot be attributed to the MSI.
$nsisProcess = Start-Process $nsis.FullName -ArgumentList "/S" -Wait -PassThru
if ($nsisProcess.ExitCode -ne 0) { throw "Silent NSIS install failed: $($nsisProcess.ExitCode)" }
$userRuntime = Join-Path $env:LOCALAPPDATA "muniment\muniment-runtime.exe"
if (-not (Test-Path $userRuntime)) { throw "NSIS runtime not found at $userRuntime" }
$nsisUninstaller = Join-Path $env:LOCALAPPDATA "muniment\uninstall.exe"
if (-not (Test-Path $nsisUninstaller)) { throw "NSIS uninstaller not found at $nsisUninstaller" }
$nsisUninstall = Start-Process $nsisUninstaller -ArgumentList "/S" -Wait -PassThru
if ($nsisUninstall.ExitCode -ne 0) { throw "Silent NSIS uninstall failed: $($nsisUninstall.ExitCode)" }
if (Test-Path $userRuntime) { throw "NSIS runtime remains after uninstall at $userRuntime" }

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
    $userRegistration = @(Get-MsiRegistrations $regularMsi[0].FullName)
    Write-Host "The regular MSI has $($userRegistration.Count) Windows Installer registrations."
    foreach ($registration in $userRegistration) {
      Write-Host "The MSI registration has ProductCode=$($registration.ProductCode) context=$($registration.Context) SID=$($registration.UserSid)."
    }
    $registryRegistration = Get-PerUserMsiRegistration $userProductCode $sessionSid
    Write-PerUserMsiRegistration $registryRegistration "installed"
  }
} finally {
  Remove-Item -LiteralPath $userMsiLog -Force
}
$userKey = "HKCU:\Software\Muniment\muniment"
try {
  # Windows Installer can store a per-user uninstall entry under HKLM. Its product context defines the install scope.
  Assert-UserMsiRegistration $userRegistration $sessionSid
  Assert-PerUserMsiRegistration $registryRegistration $env:LOCALAPPDATA
  if (($hkcu.Count + $hklm.Count) -ne 1) {
    throw "The regular MSI must have exactly one uninstall registration."
  }
  if (Test-Path $machineKey) { throw "The regular MSI wrote application registration under HKLM." }
  if ((Get-ItemPropertyValue $userKey InstallDir).TrimEnd('\') -ne (Split-Path $userRuntime)) {
    throw "The regular MSI did not register its LocalAppData install under HKCU."
  }
  if (-not (Test-Path $userRuntime)) { throw "Regular MSI runtime not found at $userRuntime" }
} finally {
  try {
    Invoke-Msi "/x" $regularMsi[0].FullName "Silent regular MSI uninstall"
  } finally {
    $registryRegistration = Get-PerUserMsiRegistration $userProductCode $sessionSid
    Write-PerUserMsiRegistration $registryRegistration "uninstalled"
    Assert-PerUserMsiRegistration $registryRegistration $env:LOCALAPPDATA -Absent
    if (@(Get-MsiRegistrations $regularMsi[0].FullName).Count -ne 0) {
      throw "Windows Installer registration remains after regular MSI uninstall."
    }
    if (Test-Path $userKey) { throw "Application registration remains after regular MSI uninstall." }
    if (Test-Path $userRuntime) { throw "The regular MSI runtime remains after uninstall." }
  }
}

Write-Host "Windows silent installer verification OK"
