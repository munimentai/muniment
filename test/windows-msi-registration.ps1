function Get-MsiProductCode([string]$Package) {
  $installer = $database = $view = $record = $null
  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase((Resolve-Path -LiteralPath $Package).Path, 0)
    $view = $database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property`` = 'ProductCode'")
    $view.Execute()
    $record = $view.Fetch()
    if ($null -eq $record) { throw "The MSI has no ProductCode." }
    $code = $record.StringData(1)
    ConvertTo-PackedProductCode $code | Out-Null
    return $code
  } finally {
    if ($null -ne $view) { $view.Close() }
    foreach ($comObject in @($record, $view, $database, $installer)) {
      if ($null -ne $comObject) { [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($comObject) }
    }
  }
}

function ConvertTo-PackedProductCode([string]$ProductCode) {
  if ($ProductCode -notmatch '^\{[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\}$') {
    throw "The MSI ProductCode is invalid: $ProductCode"
  }
  $hex = $ProductCode.Replace('-', '').Trim('{', '}').ToUpperInvariant()
  $order = @(7..0) + @(11..8) + @(15..12)
  for ($index = 16; $index -lt 32; $index += 2) { $order += @(($index + 1), $index) }
  return -join ($order | ForEach-Object { $hex[$_] })
}

function Test-MsiUserLocation([string]$Location, [string]$LocalAppData) {
  if ([string]::IsNullOrWhiteSpace($Location) -or [string]::IsNullOrWhiteSpace($LocalAppData)) { return $false }
  # Reject relative paths before normalization can resolve them against the harness directory.
  if ($Location -notmatch '^[a-zA-Z]:\\' -or $LocalAppData -notmatch '^[a-zA-Z]:\\') { return $false }
  try {
    $root = [IO.Path]::GetFullPath($LocalAppData).TrimEnd('\') + '\'
    $path = [IO.Path]::GetFullPath($Location).TrimEnd('\')
    return $path.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)
  } catch { return $false }
}

function Get-PerUserMsiRegistration([string]$ProductCode, [string]$Sid) {
  $packed = ConvertTo-PackedProductCode $ProductCode
  if ($Sid -notmatch '^S-1-\d+(-\d+)+$') { throw "The installer SID is invalid: $Sid" }
  $installerRoot = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Installer'
  $paths = [ordered]@{
    userProduct = "HKCU:\Software\Microsoft\Installer\Products\$packed"
    userData = "$installerRoot\UserData\$Sid\Products\$packed"
    managed = "$installerRoot\Managed\$Sid\Installer\Products\$packed"
    machineProduct = "HKLM:\SOFTWARE\Classes\Installer\Products\$packed"
  }
  $keys = [ordered]@{}
  foreach ($name in $paths.Keys) {
    $keys[$name] = @{ path = $paths[$name]; present = [bool](Test-Path -LiteralPath $paths[$name]) }
  }
  $propertiesPath = "$($paths.userData)\InstallProperties"
  $properties = if (Test-Path -LiteralPath $propertiesPath) { Get-ItemProperty -LiteralPath $propertiesPath } else { $null }
  $location = if ($properties -and $properties.PSObject.Properties['InstallLocation']) { [string]$properties.InstallLocation } else { '' }
  $icon = if ($properties -and $properties.PSObject.Properties['DisplayIcon']) { [string]$properties.DisplayIcon } else { '' }
  $uninstall = [ordered]@{}
  foreach ($hive in @('HKCU:', "Registry::HKEY_USERS\$Sid", 'HKLM:')) {
    $entries = @(foreach ($suffix in @('Software\Microsoft\Windows\CurrentVersion\Uninstall', 'Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall')) {
      $path = "$hive\$suffix\$ProductCode"
      if (Test-Path -LiteralPath $path) {
        $entry = Get-ItemProperty -LiteralPath $path
        @{ path = $path; InstallLocation = $(if ($entry.PSObject.Properties['InstallLocation']) { [string]$entry.InstallLocation } else { '' }) }
      }
    })
    $uninstall[$hive] = $entries
  }
  return @{ ProductCode = $ProductCode; Sid = $Sid; keys = $keys; InstallLocation = $location; DisplayIcon = $icon; uninstall = $uninstall }
}

function Write-PerUserMsiRegistration($Registration, [string]$Phase) {
  $prefix = "per-user MSI $Phase ProductCode=$($Registration.ProductCode)"
  foreach ($name in $Registration.keys.Keys) {
    $key = $Registration.keys[$name]
    Write-Host "$prefix $($key.path) present=$([int]$key.present)"
  }
  Write-Host "$prefix InstallLocation=$($Registration.InstallLocation)"
  $uninstall = $Registration.uninstall
  $sid = $Registration.Sid
  Write-Host "$prefix hkcu=$(@($uninstall['HKCU:']).Count) HKU\$sid=$(@($uninstall["Registry::HKEY_USERS\$sid"]).Count) hklm=$(@($uninstall['HKLM:']).Count)"
  foreach ($hive in $uninstall.Keys) {
    foreach ($entry in $uninstall[$hive]) { Write-Host "$prefix $($entry.path) InstallLocation=$($entry.InstallLocation)" }
  }
}

function Assert-PerUserMsiRegistration($Registration, [string]$LocalAppData, [switch]$Absent) {
  $invalid = @()
  foreach ($name in $Registration.keys.Keys) {
    $expected = -not $Absent -and $name -in @('userProduct', 'userData')
    if ($Registration.keys[$name].present -ne $expected) { $invalid += $Registration.keys[$name].path }
  }
  if (-not $Absent -and -not (Test-MsiUserLocation $Registration.InstallLocation $LocalAppData)) { $invalid += 'InstallLocation' }
  foreach ($hive in $Registration.uninstall.Keys) {
    foreach ($entry in $Registration.uninstall[$hive]) {
      if ($Absent -or -not (Test-MsiUserLocation $entry.InstallLocation $LocalAppData)) { $invalid += $entry.path }
    }
  }
  if ($Absent -and $Registration.InstallLocation) { $invalid += 'InstallLocation' }
  if ($invalid.Count) {
    throw "Per-user MSI registration failed for ProductCode=$($Registration.ProductCode): $($invalid -join ', ')"
  }
}
