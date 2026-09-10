function Get-MsiRegistrations($Package, $UserSid) {
  $installer = $database = $view = $record = $products = $null
  try {
    $installer = New-Object -ComObject WindowsInstaller.Installer
    $database = $installer.OpenDatabase((Resolve-Path -LiteralPath $Package).Path, 0)
    $view = $database.OpenView('SELECT `Value` FROM `Property` WHERE `Property` = ''ProductCode''')
    [void]$view.Execute()
    $record = $view.Fetch()
    if ($null -eq $record -or [string]::IsNullOrWhiteSpace($record.StringData(1))) {
      throw "The MSI has no ProductCode."
    }
    # Query all three contexts for this product and user, including the machine context.
    $products = $installer.ProductsEx($record.StringData(1), $UserSid, 7)
    foreach ($product in $products) {
      try {
        # Read the named fields through IDispatch rather than PowerShell's COM property adapter.
        # Microsoft uses these fields in Samples/Win7Samples/sysmgmt/msi/scripts/WiLstPrdEx.vbs in Windows-classic-samples.
        $type = $product.GetType()
        [PSCustomObject]@{
          ProductCode = $type.InvokeMember('ProductCode', 'GetProperty', $null, $product, $null)
          Context = [int]$type.InvokeMember('Context', 'GetProperty', $null, $product, $null)
          UserSid = $type.InvokeMember('UserSid', 'GetProperty', $null, $product, $null)
          # InstallProperty calls MsiGetProductInfoEx for this product, user, and context.
          State = [int]$type.InvokeMember('InstallProperty', 'GetProperty', $null, $product, @('State'))
        }
      } finally {
        if ([Runtime.InteropServices.Marshal]::IsComObject($product)) {
          [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($product)
        }
      }
    }
  } finally {
    if ($null -ne $view) { [void]$view.Close() }
    foreach ($comObject in @($products, $record, $view, $database, $installer)) {
      if ($null -ne $comObject -and [Runtime.InteropServices.Marshal]::IsComObject($comObject)) {
        [void][Runtime.InteropServices.Marshal]::FinalReleaseComObject($comObject)
      }
    }
  }
}

function Assert-PerUserMsiRegistration($Registrations, $UserSid) {
  # Context 2 means per-user unmanaged. State 5 means installed, not merely advertised.
  if ($Registrations.Count -ne 1 -or $Registrations[0].Context -ne 2 -or
      $Registrations[0].UserSid -ne $UserSid -or $Registrations[0].State -ne 5) {
    throw "The MSI must register one installed, unmanaged product for the current user and none for the machine."
  }
}
