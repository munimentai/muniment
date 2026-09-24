# Build the Go reader sidecar to the path the caller names.
#
# The desktop-ci clones carry no Go toolchain, and the tauri bundle declares the
# reader as a resource, so every lane that compiles the desktop package needs
# this binary on disk first. Go already on the clone is used as it stands, and
# the pinned toolchain is fetched only when none is there.
param([Parameter(Mandatory = $true)][string]$Output)

$ErrorActionPreference = "Stop"
# The progress bar makes Invoke-WebRequest spend minutes on a large file.
$ProgressPreference = "SilentlyContinue"
$repoRoot = Split-Path -Parent $PSScriptRoot
$goVersion = "go1.24.13"
# The SHA-256 go.dev/dl publishes for the Windows amd64 archive.
$goSha256 = "40b16bc8f00540a2cb02dff4de72b73e966fdd8d65f95e33d8e4080b48a2459a"

$env:PATH = "$env:PATH;C:\Program Files\Go\bin;$env:LOCALAPPDATA\Go\bin"

if (-not (Get-Command go -ErrorAction SilentlyContinue)) {
  $root = Join-Path $env:LOCALAPPDATA "go-toolchain"
  $installed = Join-Path $root "go\bin\go.exe"
  if (-not (Test-Path $installed)) {
    $archive = Join-Path $env:TEMP "$goVersion.windows-amd64.zip"
    Invoke-WebRequest -UseBasicParsing `
      -Uri "https://go.dev/dl/$goVersion.windows-amd64.zip" -OutFile $archive
    if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $goSha256) {
      Remove-Item -Force $archive
      throw "$goVersion checksum does not match"
    }
    if (Test-Path $root) { Remove-Item -Recurse -Force $root }
    # Expand-Archive takes minutes on the toolchain, and this takes seconds.
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory($archive, $root)
    Remove-Item -Force $archive
  }
  $env:PATH = "$env:PATH;$(Join-Path $root 'go\bin')"
}

go version
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$target = if ([System.IO.Path]::IsPathRooted($Output)) { $Output } else { Join-Path $repoRoot $Output }
$env:CGO_ENABLED = "0"
go build -C (Join-Path $repoRoot "src-tauri\reader") -trimpath -ldflags "-s -w" -o $target .
exit $LASTEXITCODE
