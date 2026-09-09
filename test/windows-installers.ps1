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
function Get-UninstallEntries([ValidateSet("HKCU", "HKLM")][string]$Hive = "HKCU") {
  $roots = @(
    "$Hive`:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
    "$Hive`:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
  )
  return @($roots | Where-Object { Test-Path $_ } | ForEach-Object { Get-ChildItem $_ | ForEach-Object {
    $properties = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
    if ($properties) {
      $displayName = $properties.PSObject.Properties["DisplayName"]
      [pscustomobject]@{ PSChildName = $_.PSChildName; PSPath = $_.PSPath; DisplayName = $(if ($displayName) { $displayName.Value } else { $null }) }
    }
  } })
}

function Get-MunimentRegistrations([ValidateSet("HKCU", "HKLM")][string]$Hive = "HKLM") {
  return @(Get-UninstallEntries $Hive | Where-Object { $_.DisplayName -eq "muniment" })
}

function Assert-MunimentRegistrations([int]$UserCount, [int]$MachineCount, [string]$Description) {
  $userRegistrations = @(Get-MunimentRegistrations "HKCU")
  $machineRegistrations = @(Get-MunimentRegistrations "HKLM")
  $entries = [ordered]@{ hkcu = $userRegistrations; hklm = $machineRegistrations } | ConvertTo-Json -Depth 4 -Compress
  Write-Host "$Description`: hkcu=$($userRegistrations.Count) hklm=$($machineRegistrations.Count) entries=$entries"
  if ($userRegistrations.Count -ne $UserCount -or $machineRegistrations.Count -ne $MachineCount) {
    throw "$Description requires hkcu=$UserCount hklm=$MachineCount."
  }
}

Invoke-Msi "/i" $upgradeBaseMsi "Silent base MSI install"
# PowerShell 5.1 gives a single PSCustomObject no Count property. Keep the results as arrays.
$baseRegistration = @(Get-MunimentRegistrations)
if ($baseRegistration.Count -ne 1) { throw "Base MSI is not registered exactly once under HKLM uninstall registration" }
$oldProductCode = $baseRegistration[0].PSChildName

Invoke-Msi "/i" $machineMsi.FullName "Silent MSI in-place upgrade"
$newRegistration = @(Get-MunimentRegistrations)
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
if (@(Get-MunimentRegistrations).Count -ne 0) {
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

Assert-MunimentRegistrations 0 0 "Registration before per-user MSI install"
Invoke-Msi "/i" $regularMsi[0].FullName "Silent per-user MSI install"
Assert-MunimentRegistrations 1 0 "Per-user MSI registration"
$userKey = "HKCU:\Software\Muniment\muniment"
$userInstallDir = Get-ItemPropertyValue $userKey InstallDir
if ([string]::IsNullOrWhiteSpace($userInstallDir) -or -not [IO.Path]::IsPathRooted($userInstallDir)) {
  throw "The per-user MSI did not register an absolute install directory."
}
$userInstallDir = [IO.Path]::GetFullPath($userInstallDir)
$userProfile = [IO.Path]::GetFullPath($env:USERPROFILE).TrimEnd('\') + '\'
if (-not $userInstallDir.StartsWith($userProfile, [StringComparison]::OrdinalIgnoreCase)) {
  throw "The per-user MSI install directory is outside the user profile: $userInstallDir"
}
if (-not (Test-Path $userRuntime)) { throw "The per-user MSI runtime is missing at $userRuntime" }
if (-not (Test-Path -LiteralPath (Join-Path $userInstallDir "muniment-runtime.exe") -PathType Leaf)) {
  throw "The per-user MSI runtime is missing from its registered install directory: $userInstallDir"
}
Invoke-Msi "/x" $regularMsi[0].FullName "Silent per-user MSI uninstall"
Assert-MunimentRegistrations 0 0 "Registration after per-user MSI uninstall"
if (Test-Path $userKey) { throw "The per-user MSI left its application registration after uninstall." }
if (Test-Path $userRuntime) { throw "The per-user MSI left its runtime after uninstall: $userRuntime" }

Write-Host "Windows silent installer verification OK"
