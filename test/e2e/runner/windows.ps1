param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$ProgressPreference = "SilentlyContinue"

$artifacts = if ($env:DCI_ARTIFACTS_DIR) { $env:DCI_ARTIFACTS_DIR } else { "C:\dci-artifacts" }
$runRoot = Join-Path $env:TEMP ("muniment-e2e-" + [guid]::NewGuid().ToString("N"))
$raw = Join-Path $runRoot "raw"
$safe = Join-Path $runRoot "safe"
$stateRoot = Join-Path $runRoot "state"
$msi = Join-Path $runRoot "muniment-nightly.msi"
$authUrlFile = Join-Path $runRoot "auth-url"
$cleanupLog = Join-Path $runRoot "cleanup.log"
$installerLog = Join-Path $raw "installer.log"
$status = 0
$cleanupStatus = 0
$script:redacted = $true
$ready = $false
$installAttempted = $false
$installDirectory = $null
$productCode = $null
$handlerKey = "HKCU:\Software\Classes\muniment-e2e-https"
$httpsKey = "HKCU:\Software\Classes\https"

New-Item -ItemType Directory -Force $raw, $stateRoot | Out-Null
New-Item -ItemType File -Force $cleanupLog | Out-Null

function Invoke-BoundedProcess([string]$File, [string]$Arguments, [int]$TimeoutSeconds, [string]$Log) {
  $process = Start-Process $File -ArgumentList $Arguments -PassThru -RedirectStandardOutput $Log -RedirectStandardError ($Log + ".err")
  if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    throw "$File timed out"
  }
  if ($process.ExitCode -notin @(0, 3010)) { throw "$File failed with exit code $($process.ExitCode)" }
}

function Get-ProductRegistration {
  $roots = @(
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
    "HKCU:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
  )
  return @($roots | Where-Object { Test-Path $_ } | ForEach-Object {
    Get-ChildItem $_ | Where-Object { (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).DisplayName -eq "muniment" }
  })
}

function Invoke-Cleanup([string]$Name, [scriptblock]$Action) {
  try {
    & $Action *>> $cleanupLog
    Add-Content $cleanupLog "$Name`: ok"
  } catch {
    Add-Content $cleanupLog "$Name`: failed"
    $script:cleanupStatus = 1
  }
}

function Remove-AuthHandler {
  Remove-Item $handlerKey -Recurse -Force -ErrorAction SilentlyContinue
  if (Test-Path $httpsKey) {
    $current = (Get-ItemProperty $httpsKey -ErrorAction SilentlyContinue).'(default)'
    if ($current -eq "URL:muniment-e2e-https") { Remove-Item $httpsKey -Recurse -Force }
  }
}

function Finalize-Run {
  Invoke-Cleanup "stop-wdio" { Get-CimInstance Win32_Process | Where-Object CommandLine -Like '*wdio.conf.js*' | ForEach-Object { Stop-Process -Id $_.ProcessId -Force } }
  Invoke-Cleanup "stop-drivers" { Get-Process tauri-driver, msedgedriver -ErrorAction SilentlyContinue | Stop-Process -Force }
  if ($ready) {
    Invoke-Cleanup "revoke-session" { $env:MUNIMENT_E2E_CLEANUP_ONLY = "1"; Invoke-BoundedProcess "npm.cmd" "run test:e2e" 45 (Join-Path $raw "cleanup-wdio.log") }
  }
  Invoke-Cleanup "stop-app" { Get-Process muniment -ErrorAction SilentlyContinue | Stop-Process -Force }
  if ($installAttempted) {
    Invoke-Cleanup "uninstall" {
      $registration = @(Get-ProductRegistration)
      if ($registration.Count -gt 1) { throw "product is registered more than once" }
      $code = if ($registration.Count -eq 1) { $registration[0].PSChildName } else { $productCode }
      if ($code) { Invoke-BoundedProcess "msiexec.exe" "/x $code /qn /norestart" 180 (Join-Path $raw "uninstaller.log") }
    }
  }
  Invoke-Cleanup "remove-auth-handler" { Remove-AuthHandler }
  Invoke-Cleanup "remove-state" { Remove-Item $stateRoot -Recurse -Force -ErrorAction SilentlyContinue }
  Invoke-Cleanup "registration-gone" { if (@(Get-ProductRegistration).Count -ne 0) { throw "product registration remains" } }
  Invoke-Cleanup "installed-files-gone" { if ($installDirectory -and (Test-Path -LiteralPath $installDirectory)) { throw "installed files remain" } }
  Invoke-Cleanup "processes-gone" { if (Get-Process muniment, tauri-driver, msedgedriver -ErrorAction SilentlyContinue) { throw "test process remains" } }
  Copy-Item $cleanupLog (Join-Path $raw "cleanup.log") -Force -ErrorAction SilentlyContinue
  Invoke-Cleanup "redact-artifacts" {
    & node test/e2e/support/redact.mjs $raw $safe
    if ($LASTEXITCODE -ne 0) { $script:redacted = $false; throw "artifact redaction failed" }
  }
  Invoke-Cleanup "remove-raw" { Remove-Item $raw -Recurse -Force -ErrorAction SilentlyContinue }
  Invoke-Cleanup "remove-msi" { Remove-Item $msi -Force -ErrorAction SilentlyContinue }
  Invoke-Cleanup "remove-auth-url" { Remove-Item $authUrlFile -Force -ErrorAction SilentlyContinue }
  if ($script:redacted) {
    Invoke-Cleanup "publish-artifacts" { Remove-Item $artifacts -Recurse -Force -ErrorAction SilentlyContinue; Move-Item $safe $artifacts }
  } else {
    Invoke-Cleanup "suppress-artifacts" { Remove-Item $artifacts -Recurse -Force -ErrorAction SilentlyContinue; Remove-Item $safe -Recurse -Force -ErrorAction SilentlyContinue }
  }
  Remove-Item $cleanupLog -Force -ErrorAction SilentlyContinue
  Remove-Item $runRoot -Recurse -Force -ErrorAction SilentlyContinue
  if ($status -ne 0 -or $cleanupStatus -ne 0 -or -not $script:redacted) { exit 1 }
}

try {
  $sha = $env:MUNIMENT_E2E_SOURCE_SHA
  if ($sha -notmatch '^[0-9a-f]{40}$') { throw "invalid source SHA" }
  if (-not $env:GH_TOKEN -or -not $env:GITHUB_REPOSITORY -or -not $env:MUNIMENT_E2E_USERNAME -or -not $env:MUNIMENT_E2E_PASSWORD) {
    throw "required injected environment is unavailable"
  }

  # Resolve all identity checks before mutating installer or per-user state.
  $release = & gh api "repos/$($env:GITHUB_REPOSITORY)/releases/tags/nightly" | Out-String
  if ($LASTEXITCODE -ne 0) { throw "nightly release lookup failed" }
  $assetId = $release | & node test/e2e/support/asset-identity.mjs $sha windows
  if ($LASTEXITCODE -ne 0 -or $assetId -notmatch '^[1-9][0-9]*$') { throw "Windows artifact identity validation failed" }
  Invoke-WebRequest -UseBasicParsing -Headers @{ Accept = "application/octet-stream"; Authorization = "Bearer $($env:GH_TOKEN)" } `
    -Uri "https://api.github.com/repos/$($env:GITHUB_REPOSITORY)/releases/assets/$assetId" -OutFile $msi

  $installer = New-Object -ComObject WindowsInstaller.Installer
  $database = $installer.GetType().InvokeMember("OpenDatabase", "InvokeMethod", $null, $installer, @($msi, 0))
  $view = $database.GetType().InvokeMember("OpenView", "InvokeMethod", $null, $database, @("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductName'"))
  $view.GetType().InvokeMember("Execute", "InvokeMethod", $null, $view, $null)
  $record = $view.GetType().InvokeMember("Fetch", "InvokeMethod", $null, $view, $null)
  $productName = $record.GetType().InvokeMember("StringData", "GetProperty", $null, $record, 1)
  if ($productName -ne "muniment") { throw "package identity mismatch" }

  $env:APPDATA = Join-Path $stateRoot "Roaming"
  $env:LOCALAPPDATA = Join-Path $stateRoot "Local"
  $env:npm_config_cache = Join-Path $runRoot "npm-cache"
  New-Item -ItemType Directory -Force $env:APPDATA, $env:LOCALAPPDATA | Out-Null
  $installAttempted = $true
  Invoke-BoundedProcess "msiexec.exe" "/i `"$msi`" /qn /norestart" 180 $installerLog

  $registrations = @(Get-ProductRegistration)
  if ($registrations.Count -ne 1) { throw "per-user MSI is not registered exactly once" }
  $registration = Get-ItemProperty $registrations[0].PSPath
  $productCode = $registrations[0].PSChildName
  $installDirectory = $registration.InstallLocation
  $appBinary = if ($registration.DisplayIcon) { ($registration.DisplayIcon -replace '^"|"?(?:,\d+)?$', '') } else { $null }
  if (-not $appBinary -or -not (Test-Path -LiteralPath $appBinary -PathType Leaf)) {
    if (-not $installDirectory) { throw "installer metadata does not identify an install directory" }
    $matches = @(Get-ChildItem -LiteralPath $installDirectory -Filter "muniment.exe" -File -Recurse)
    if ($matches.Count -ne 1) { throw "installer metadata does not resolve one executable" }
    $appBinary = $matches[0].FullName
  }
  if (-not $installDirectory) { $installDirectory = Split-Path $appBinary -Parent }

  & npm.cmd ci --no-audit --no-fund *>> $installerLog
  if ($LASTEXITCODE -ne 0) { throw "npm dependency installation failed" }
  if (-not (Get-Command tauri-driver.exe -ErrorAction SilentlyContinue)) {
    & cargo install tauri-driver --version 2.0.5 --locked *>> $installerLog
    if ($LASTEXITCODE -ne 0) { throw "tauri-driver installation failed" }
  }

  New-Item -Path $handlerKey -Force | Out-Null
  Set-ItemProperty $handlerKey -Name '(default)' -Value 'URL:muniment-e2e-https'
  New-ItemProperty $handlerKey -Name 'URL Protocol' -Value '' -PropertyType String -Force | Out-Null
  $commandKey = Join-Path $handlerKey 'shell\open\command'
  New-Item -Path $commandKey -Force | Out-Null
  $launcher = (Resolve-Path test/e2e/support/browser-launcher.ps1).Path
  Set-ItemProperty $commandKey -Name '(default)' -Value "`"powershell.exe`" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$launcher`" `"%1`""
  Remove-Item 'HKCU:\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice' -Recurse -Force -ErrorAction SilentlyContinue
  New-Item -Path $httpsKey -Force | Out-Null
  Set-ItemProperty $httpsKey -Name '(default)' -Value 'URL:muniment-e2e-https'
  New-ItemProperty $httpsKey -Name 'URL Protocol' -Value '' -PropertyType String -Force | Out-Null
  $httpsCommandKey = Join-Path $httpsKey 'shell\open\command'
  New-Item -Path $httpsCommandKey -Force | Out-Null
  Set-ItemProperty $httpsCommandKey -Name '(default)' -Value "`"powershell.exe`" -NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$launcher`" `"%1`""

  $env:MUNIMENT_E2E_APP_BINARY = $appBinary
  $env:MUNIMENT_E2E_RAW_DIR = $raw
  $env:MUNIMENT_E2E_AUTH_URL_FILE = $authUrlFile
  $ready = $true
  $wdioLog = Join-Path $raw "wdio.log"
  $driverAppLog = Join-Path $raw "driver-app.log"
  & npm.cmd run test:e2e 1> $wdioLog 2> $driverAppLog
  if ($LASTEXITCODE -ne 0) { $status = 1 }
} catch {
  Add-Content $installerLog "runner failed: $($_.Exception.Message)"
  $status = 1
} finally {
  Finalize-Run
}
