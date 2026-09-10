$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "windows-msi-registration.ps1")

$userSid = "S-1-5-21-1-2-3-1001"
$user = [PSCustomObject]@{ Context = 2; UserSid = $userSid; State = 5 }
$machine = [PSCustomObject]@{ Context = 4; UserSid = ""; State = 5 }

# Windows Installer's uninstall entry can occupy either hive without changing the product context.
Assert-PerUserMsiRegistration @($user) $userSid

$cases = @(
  @{ Name = "empty"; Registrations = @() },
  @{ Name = "missing"; Registrations = $null },
  @{ Name = "machine"; Registrations = @($machine) },
  @{ Name = "user and machine"; Registrations = @($user, $machine) },
  @{ Name = "duplicate user"; Registrations = @($user, $user) },
  @{ Name = "managed user"; Registrations = @([PSCustomObject]@{ Context = 1; UserSid = $userSid; State = 5 }) },
  @{ Name = "unknown context"; Registrations = @([PSCustomObject]@{ Context = 0; UserSid = $userSid; State = 5 }) },
  @{ Name = "combined contexts"; Registrations = @([PSCustomObject]@{ Context = 7; UserSid = $userSid; State = 5 }) },
  @{ Name = "wrong user"; Registrations = @([PSCustomObject]@{ Context = 2; UserSid = "S-1-5-21-1-2-3-1002"; State = 5 }) },
  @{ Name = "missing user"; Registrations = @([PSCustomObject]@{ Context = 2; UserSid = ""; State = 5 }) },
  @{ Name = "advertised"; Registrations = @([PSCustomObject]@{ Context = 2; UserSid = $userSid; State = 1 }) },
  @{ Name = "unknown state"; Registrations = @([PSCustomObject]@{ Context = 2; UserSid = $userSid; State = -1 }) },
  @{ Name = "missing fields"; Registrations = @([PSCustomObject]@{}) }
)
foreach ($case in $cases) {
  $rejected = $false
  try {
    Assert-PerUserMsiRegistration $case.Registrations $userSid
  } catch {
    $rejected = $true
  }
  if (-not $rejected) { throw "The MSI registration check accepted the $($case.Name) case." }
}
# Model the COM boundary without replacing Get-MsiRegistrations or its conversion.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.CompilerServices;
namespace MsiRegistrationFixture {
  public class Product {
    public string ProductCode { get; set; }
    public int Context { get; set; }
    public string UserSid { get; set; }
    public string InstallState;
    public bool FailStateQuery;
    [IndexerName("InstallProperty")]
    public string this[string property] {
      get {
        if (property != "State") throw new Exception("The query requested an unknown install property.");
        if (FailStateQuery) throw new Exception("The product state query failed.");
        return InstallState;
      }
    }
  }
  public class Record {
    public string Code;
    public string StringData(int field) {
      if (field != 1) throw new Exception("The query requested an unknown record field.");
      return Code;
    }
  }
  public class View {
    public Record Record;
    public bool Executed;
    public bool Closed;
    public int Execute() { Executed = true; return 0; }
    public Record Fetch() {
      if (!Executed) throw new Exception("The view must execute before Fetch.");
      return Record;
    }
    public int Close() { Closed = true; return 0; }
  }
  public class Database {
    public View View;
    public View OpenView(string query) {
      if (query != "SELECT `Value` FROM `Property` WHERE `Property` = 'ProductCode'")
        throw new Exception("The query must read the product code.");
      return View;
    }
  }
  public class Installer {
    public Database Database;
    public object[] Products = new object[0];
    public string ExpectedCode;
    public string ExpectedSid;
    public int Enumerations;
    public Database OpenDatabase(string path, int mode) {
      if (!System.IO.File.Exists(path) || mode != 0) throw new Exception("The query must open an existing database in read-only mode.");
      return Database;
    }
    public object[] ProductsEx(string code, string sid, int contexts) {
      if (code != ExpectedCode || sid != ExpectedSid || contexts != 7)
        throw new Exception("The query must include the product, current user, and all three contexts.");
      Enumerations++;
      return Products;
    }
  }
}
'@

& {
  $code = "{11111111-2222-3333-4444-555555555555}"
  $product = [MsiRegistrationFixture.Product]::new()
  $product.ProductCode = $code
  $product.Context = 2
  $product.UserSid = $userSid
  $product.InstallState = "5"
  # Hide the adapted fields while reflection still exposes the underlying properties.
  foreach ($field in @("ProductCode", "Context", "UserSid")) {
    $product | Add-Member -MemberType NoteProperty -Name $field -Value $null -Force
  }
  $record = [MsiRegistrationFixture.Record]::new()
  $record.Code = $code
  $view = [MsiRegistrationFixture.View]::new()
  $view.Record = $record
  $database = [MsiRegistrationFixture.Database]::new()
  $database.View = $view
  $installer = [MsiRegistrationFixture.Installer]::new()
  $installer.Database = $database
  $installer.ExpectedCode = $code
  $installer.ExpectedSid = $userSid
  $installer.Products = @($product)

  $fixtureInstaller = $installer
  # Replace only COM activation. Keep the real query, field conversion, and cleanup path.
  function New-Object($ComObject) {
    if ($ComObject -ne "WindowsInstaller.Installer") { throw "The query must activate WindowsInstaller.Installer." }
    return $fixtureInstaller
  }

  $registrations = @(Get-MsiRegistrations $PSCommandPath $userSid)
  if ($registrations.Count -ne 1 -or $registrations[0].ProductCode -cne $code -or
      $registrations[0].Context -ne 2 -or $registrations[0].UserSid -cne $userSid -or
      $registrations[0].State -ne 5 -or -not $view.Closed -or $installer.Enumerations -ne 1) {
    throw "The MSI query must return exactly one complete registration and close the view."
  }
  Assert-PerUserMsiRegistration $registrations $userSid

  $machineProduct = [MsiRegistrationFixture.Product]::new()
  $machineProduct.ProductCode = $code
  $machineProduct.Context = 4
  $machineProduct.UserSid = ""
  $machineProduct.InstallState = "1"
  $installer.Products = @($product, $machineProduct)
  $registrations = @(Get-MsiRegistrations $PSCommandPath $userSid)
  if ($registrations.Count -ne 2 -or $registrations[0].State -ne 5 -or
      $registrations[1].ProductCode -cne $code -or $registrations[1].Context -ne 4 -or
      $registrations[1].UserSid -cne "" -or $registrations[1].State -ne 1) {
    throw "The MSI query must preserve each product context, user, and install state."
  }
  $rejected = $false
  try { Assert-PerUserMsiRegistration $registrations $userSid } catch { $rejected = $true }
  if (-not $rejected) { throw "The MSI check accepted a machine registration beside the user registration." }

  $installer.Products = @()
  $view.Closed = $false
  if (@(Get-MsiRegistrations $PSCommandPath $userSid).Count -ne 0 -or -not $view.Closed) {
    throw "The MSI query must return zero objects after uninstall and close the view."
  }

  foreach ($badCode in @($null, "", " ")) {
    $record.Code = $badCode
    $view.Closed = $false
    $enumerations = $installer.Enumerations
    $rejected = $false
    try { Get-MsiRegistrations $PSCommandPath $userSid } catch { $rejected = $true }
    if (-not $rejected -or -not $view.Closed -or $installer.Enumerations -ne $enumerations) {
      throw "The MSI query must reject a missing product code before enumeration and close the view."
    }
  }
  $view.Record = $null
  $rejected = $false
  try { Get-MsiRegistrations $PSCommandPath $userSid } catch { $rejected = $true }
  if (-not $rejected) { throw "The MSI query accepted a missing product code record." }

  $view.Record = $record
  $record.Code = $code
  $installer.Products = @($product)
  $product.FailStateQuery = $true
  $view.Closed = $false
  $rejected = $false
  try { Get-MsiRegistrations $PSCommandPath $userSid } catch { $rejected = $true }
  if (-not $rejected -or -not $view.Closed) {
    throw "The MSI query must report a state query failure and close the view."
  }
}
Write-Host "MSI registration tests passed."
