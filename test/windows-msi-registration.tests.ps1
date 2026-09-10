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
Write-Host "MSI registration tests passed."
