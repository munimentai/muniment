# Build the Go reader sidecar to the path the caller names.
#
# The desktop-ci clones carry no Go toolchain, and the tauri bundle declares the
# reader as a resource, so every lane that compiles the desktop package needs
# this binary on disk first. Go already on the clone is used as it stands, and
# the pinned toolchain is fetched only when none is there.
param([Parameter(Mandatory = $true)][string]$Output)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$goVersion = "go1.24.13"

$env:PATH = "$env:PATH;C:\Program Files\Go\bin;$env:LOCALAPPDATA\Go\bin"

if (-not (Get-Command go -ErrorAction SilentlyContinue)) {
  $archive = Join-Path $env:TEMP "$goVersion.windows-amd64.zip"
  Invoke-WebRequest -UseBasicParsing `
    -Uri "https://go.dev/dl/$goVersion.windows-amd64.zip" -OutFile $archive
  $root = Join-Path $env:LOCALAPPDATA "go-toolchain"
  if (Test-Path $root) { Remove-Item -Recurse -Force $root }
  Expand-Archive -Path $archive -DestinationPath $root -Force
  Remove-Item -Force $archive
  $env:PATH = "$env:PATH;$(Join-Path $root 'go\bin')"
}

go version
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$target = if ([System.IO.Path]::IsPathRooted($Output)) { $Output } else { Join-Path $repoRoot $Output }
$env:CGO_ENABLED = "0"
go build -C (Join-Path $repoRoot "src-tauri\reader") -trimpath -ldflags "-s -w" -o $target .
exit $LASTEXITCODE
