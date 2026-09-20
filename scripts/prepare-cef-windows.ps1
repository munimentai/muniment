$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$toolsRoot = Join-Path $env:LOCALAPPDATA 'muniment-cef-build-tools'
New-Item -ItemType Directory -Force $toolsRoot | Out-Null
function Install-Tool([string]$Name,[string]$Url,[string]$Sha256,[string]$Executable) {
  $directory=Join-Path $toolsRoot $Name
  $binary=Join-Path $directory $Executable
  if (!(Test-Path $binary)) {
    $archive=Join-Path $toolsRoot "$Name.zip"
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $archive
    if ((Get-FileHash $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $Sha256) {throw "$Name checksum does not match"}
    New-Item -ItemType Directory -Force $directory | Out-Null
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::ExtractToDirectory($archive,$directory)
    Remove-Item $archive
  }
  $env:PATH=(Split-Path -Parent $binary)+';'+$env:PATH
}
Install-Tool 'cmake-4.4.3' 'https://github.com/Kitware/CMake/releases/download/v4.4.3/cmake-4.4.3-windows-x86_64.zip' '4d52ebab7193a698651639ed80d8d04fd903358843572cf44c7fd234cb7c26ab' 'cmake-4.4.3-windows-x86_64/bin/cmake.exe'
Install-Tool 'ninja-1.13.2' 'https://github.com/ninja-build/ninja/releases/download/v1.13.2/ninja-win.zip' '07fc8261b42b20e71d1720b39068c2e14ffcee6396b76fb7a795fb460b78dc65' 'ninja.exe'
