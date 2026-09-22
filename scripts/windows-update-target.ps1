$ErrorActionPreference = 'Stop'
$directory = [IO.Path]::GetFullPath($env:MUNIMENT_UPDATE_INSTALL_DIR).TrimEnd('\')
$targets = @()
foreach ($scope in @('user', 'machine')) {
  $root = if ($scope -eq 'user') { 'HKCU:' } else { 'HKLM:' }
  $entries = Get-ItemProperty "$root\Software\Microsoft\Windows\CurrentVersion\Uninstall\*" -ErrorAction SilentlyContinue
  foreach ($entry in $entries) {
    if ($entry.DisplayName -ne 'muniment' -or -not $entry.InstallLocation) { continue }
    $location = [IO.Path]::GetFullPath($entry.InstallLocation).TrimEnd('\')
    if (-not [string]::Equals($location, $directory, [StringComparison]::OrdinalIgnoreCase)) { continue }
    if ($entry.WindowsInstaller -eq 1) { $targets += "windows-x86_64-msi-$scope" }
    elseif ($entry.DisplayName -eq 'muniment' -and $entry.UninstallString -match 'uninstall\.exe') { $targets += 'windows-x86_64-nsis' }
  }
}
if ($targets.Count -ne 1) { throw 'Expected exactly one installed Muniment package.' }
Write-Output $targets[0]
