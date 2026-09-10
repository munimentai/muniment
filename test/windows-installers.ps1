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
  foreach ($name in @("ALLUSERS", "MSIINSTALLPERUSER", "UserSID", "LogonUser")) {
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

$userMsiLog = [IO.Path]::GetTempFileName()
try {
  try {
    Invoke-Msi "/i" $regularMsi[0].FullName "Silent regular MSI install" $userMsiLog
  } finally {
    Write-MsiScopeLog $userMsiLog
    $sessionSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $hkcu = @(Get-UserRegistrations "HKCU:")
    $hku = @(Get-UserRegistrations "Registry::HKEY_USERS\$sessionSid")
    $hklm = @(Get-MunimentRegistrations)
    Write-Host "per-user MSI registration: session SID=$sessionSid HKU\$sessionSid=$($hku.Count) hkcu=$($hkcu.Count) hklm=$($hklm.Count)"
  }
} finally {
  Remove-Item -LiteralPath $userMsiLog -Force
}
if ($hkcu.Count -ne 1 -or $hklm.Count -ne 0) {
  throw "Per-user MSI registration requires hkcu=1 hklm=0: hkcu=$($hkcu.Count) hklm=$($hklm.Count)"
}
if (-not (Test-Path $userRuntime)) { throw "Regular MSI runtime not found at $userRuntime" }
Invoke-Msi "/x" $regularMsi[0].FullName "Silent regular MSI uninstall"

Write-Host "Windows silent installer verification OK"
