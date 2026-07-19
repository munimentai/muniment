param([Parameter(Mandatory = $true)][string]$Url)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if (-not $env:MUNIMENT_E2E_AUTH_URL_FILE) {
  throw "auth URL destination is unavailable"
}
& node (Join-Path $PSScriptRoot "capture-auth-url.mjs") $env:MUNIMENT_E2E_AUTH_URL_FILE $Url
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
