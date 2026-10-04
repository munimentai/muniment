$ErrorActionPreference = 'Stop'
$directory = [IO.Path]::GetFullPath($env:MUNIMENT_UPDATE_INSTALL_DIR).TrimEnd('\')

function Get-MsiUpdateScope($ProductCode) {
  if ($ProductCode -notmatch '^\{[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\}$') {
    throw 'The installed MSI identity is invalid.'
  }
  $installer = $products = $null
  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    # An empty SID queries the current user. Include the machine context to reject ambiguity.
    $products = $installer.ProductsEx($ProductCode, '', 7)
    $scopes = @(foreach ($product in $products) {
      try {
        $type = $product.GetType()
        $context = [int]$type.InvokeMember('Context', 'GetProperty', $null, $product, $null)
        $state = [int]$type.InvokeMember('InstallProperty', 'GetProperty', $null, $product, @('State'))
        $location = [string]$type.InvokeMember('InstallProperty', 'GetProperty', $null, $product, @('InstallLocation'))
        if ($state -ne 5 -or [string]::IsNullOrWhiteSpace($location) -or
            -not [string]::Equals([IO.Path]::GetFullPath($location).TrimEnd('\'), $directory, [StringComparison]::OrdinalIgnoreCase)) {
          throw 'The installed MSI registration is invalid.'
        }
        if ($context -eq 2) { 'user' }
        elseif ($context -eq 4) { 'machine' }
        else { throw 'The installed MSI context is unsupported.' }
      } finally {
        if ([Runtime.InteropServices.Marshal]::IsComObject($product)) {
          [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($product)
        }
      }
    })
    if ($scopes.Count -ne 1) { throw 'The MSI must have exactly one installed context.' }
    return $scopes[0]
  } finally {
    foreach ($comObject in @($products, $installer)) {
      if ($null -ne $comObject -and [Runtime.InteropServices.Marshal]::IsComObject($comObject)) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($comObject)
      }
    }
  }
}

$targets = @()
foreach ($scope in @('user', 'machine')) {
  $root = if ($scope -eq 'user') { 'HKCU:' } else { 'HKLM:' }
  $entries = Get-ItemProperty "$root\Software\Microsoft\Windows\CurrentVersion\Uninstall\*" -ErrorAction SilentlyContinue
  foreach ($entry in $entries) {
    if ($entry.DisplayName -ne 'muniment' -or -not $entry.InstallLocation) { continue }
    $location = [IO.Path]::GetFullPath($entry.InstallLocation).TrimEnd('\')
    if (-not [string]::Equals($location, $directory, [StringComparison]::OrdinalIgnoreCase)) { continue }
    # Windows Installer can put a per-user uninstall entry under HKLM.
    if ($entry.WindowsInstaller -eq 1) { $targets += "windows-x86_64-msi-$(Get-MsiUpdateScope $entry.PSChildName)" }
    elseif ($entry.DisplayName -eq 'muniment' -and $entry.UninstallString -match 'uninstall\.exe') { $targets += 'windows-x86_64-nsis' }
  }
}
if ($targets.Count -ne 1) { throw 'Expected exactly one installed Muniment package.' }
Write-Output $targets[0]
