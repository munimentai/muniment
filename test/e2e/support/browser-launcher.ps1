param([Parameter(Mandatory = $true)][string]$Url)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $Url.StartsWith("https://", [System.StringComparison]::Ordinal)) {
  exit 64
}
if (-not $env:MUNIMENT_E2E_AUTH_URL_FILE) {
  throw "auth URL destination is unavailable"
}
[System.IO.File]::WriteAllText($env:MUNIMENT_E2E_AUTH_URL_FILE, $Url, [System.Text.UTF8Encoding]::new($false))
