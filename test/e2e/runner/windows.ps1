param()

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$ProgressPreference = "SilentlyContinue"

$artifacts = $null
$runRoot = $null
$raw = $null
$safe = $null
$stateRoot = $null
$msi = $null
$authUrlFile = $null
$cleanupLog = $null
$installerLog = $null
$status = 0
$cleanupStatus = 0
$script:redacted = $true
$ready = $false
$installAttempted = $false
$installDirectory = $null
$productCode = $null
$handlerKey = "HKCU:\Software\Classes\muniment-e2e-https"
$httpsKey = "HKCU:\Software\Classes\https"
$testRegistration = $null
$testProcess = $null

function Invoke-BoundedProcess([string]$File, [string]$Arguments, [int]$TimeoutSeconds, [string]$Log) {
  $process = Start-Process $File -ArgumentList $Arguments -PassThru -RedirectStandardOutput $Log -RedirectStandardError ($Log + ".err")
  if (-not $process.WaitForExit($TimeoutSeconds * 1000)) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    throw "$File timed out"
  }
  if ($process.ExitCode -notin @(0, 3010)) { throw "$File failed with exit code $($process.ExitCode)" }
}

function Get-UninstallEntries {
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1") {
    if ($testRegistration -and (Test-Path -LiteralPath $testRegistration)) {
      return @([pscustomobject]@{ PSChildName = "test-product"; DisplayName = (Get-Content -LiteralPath $testRegistration -Raw) })
    }
    return @()
  }
  $roots = @(
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
    "HKCU:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
  )
  return @($roots | Where-Object { Test-Path $_ } | ForEach-Object { Get-ChildItem $_ | ForEach-Object {
    $properties = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
    if ($properties) { [pscustomobject]@{ PSChildName = $_.PSChildName; PSPath = $_.PSPath; DisplayName = $properties.DisplayName } }
  } })
}

function Get-ProductRegistration {
  return @(Get-UninstallEntries | Where-Object { $_.DisplayName -eq "muniment" })
}

function Get-HarnessProcesses {
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1") {
    if ($testProcess -and (Test-Path -LiteralPath $testProcess)) { return @([pscustomobject]@{ ProcessName = "muniment" }) }
    return @()
  }
  return @(Get-Process muniment, tauri-driver, msedgedriver -ErrorAction SilentlyContinue)
}

function Invoke-Cleanup([string]$Name, [scriptblock]$Action) {
  $phaseStatus = 0
  try {
    if ($env:MUNIMENT_E2E_FINALIZER_TEST_LEDGER) { Add-Content $env:MUNIMENT_E2E_FINALIZER_TEST_LEDGER $Name }
    if ($env:MUNIMENT_E2E_FINALIZER_TEST_FAIL -eq $Name) {
      if ($Name -eq "redact-artifacts") { $script:redacted = $false }
      if ($Name -eq "publish-artifacts" -and $env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1") {
        New-Item -ItemType Directory -Force $artifacts | Out-Null
        Set-Content (Join-Path $artifacts "partial-publication") "unsafe"
      }
      throw "injected $Name failure"
    }
    $testFilePhases = @("uninstall", "registration-gone", "installed-files-gone", "processes-gone", "redact-artifacts", "remove-raw", "remove-msi", "remove-auth-url", "publish-artifacts", "suppress-artifacts")
    if ($env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1" -and $Name -notin $testFilePhases) { return }
    if ($cleanupLog) { & $Action *>> $cleanupLog; Add-Content $cleanupLog "$Name`: ok" } else { & $Action | Out-Null }
  } catch {
    $phaseStatus = 1
    if ($cleanupLog) { Add-Content $cleanupLog "$Name`: failed" -ErrorAction SilentlyContinue }
    $script:cleanupStatus = 1
  } finally {
    if ($env:MUNIMENT_E2E_FINALIZER_TEST_STATUS_LEDGER) {
      Add-Content $env:MUNIMENT_E2E_FINALIZER_TEST_STATUS_LEDGER "$Name`t$phaseStatus" -ErrorAction SilentlyContinue
    }
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
      if ($env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1") {
        if ($env:MUNIMENT_E2E_FINALIZER_TEST_REMAIN_REGISTRATION -ne "1" -and $testRegistration) { Remove-Item $testRegistration -Force }
        if ($env:MUNIMENT_E2E_FINALIZER_TEST_REMAIN_FILES -ne "1" -and $installDirectory) { Remove-Item $installDirectory -Recurse -Force }
      } elseif ($code) { Invoke-BoundedProcess "msiexec.exe" "/x $code /qn /norestart" 180 (Join-Path $raw "uninstaller.log") }
    }
  }
  Invoke-Cleanup "registration-gone" { if (@(Get-ProductRegistration).Count -ne 0) { throw "product registration remains" } }
  Invoke-Cleanup "installed-files-gone" { if ($installDirectory -and (Test-Path -LiteralPath $installDirectory)) { throw "installed files remain" } }
  Invoke-Cleanup "remove-auth-handler" { Remove-AuthHandler }
  Invoke-Cleanup "remove-state" { if ($stateRoot) { Remove-Item $stateRoot -Recurse -Force -ErrorAction SilentlyContinue } }
  Invoke-Cleanup "processes-gone" {
    if (@(Get-HarnessProcesses).Count -ne 0) { throw "test process remains" }
  }
  if ($cleanupLog -and $raw) { Copy-Item $cleanupLog (Join-Path $raw "cleanup.log") -Force -ErrorAction SilentlyContinue }
  Invoke-Cleanup "redact-artifacts" {
    if (-not $raw -or -not (Test-Path $raw)) { throw "raw staging is unavailable" }
    & node test/e2e/support/redact.mjs $raw $safe
    if ($LASTEXITCODE -ne 0) { $script:redacted = $false; throw "artifact redaction failed" }
  }
  Invoke-Cleanup "remove-raw" { if ($raw) { Remove-Item $raw -Recurse -Force -ErrorAction SilentlyContinue } }
  Invoke-Cleanup "remove-msi" { if ($msi) { Remove-Item $msi -Force -ErrorAction SilentlyContinue } }
  Invoke-Cleanup "remove-auth-url" { if ($authUrlFile) { Remove-Item $authUrlFile -Force -ErrorAction SilentlyContinue } }
  if ($script:redacted) {
    $publicationStatus = $cleanupStatus
    Invoke-Cleanup "publish-artifacts" { if (-not $safe -or -not (Test-Path $safe)) { throw "safe staging is unavailable" }; Remove-Item $artifacts -Recurse -Force -ErrorAction SilentlyContinue; Move-Item $safe $artifacts }
    if ($cleanupStatus -ne $publicationStatus) { Invoke-Cleanup "suppress-artifacts" { Remove-Item $artifacts -Recurse -Force -ErrorAction SilentlyContinue; if ($safe) { Remove-Item $safe -Recurse -Force -ErrorAction SilentlyContinue } } }
  } else {
    Invoke-Cleanup "suppress-artifacts" { if ($artifacts) { Remove-Item $artifacts -Recurse -Force -ErrorAction SilentlyContinue }; if ($safe) { Remove-Item $safe -Recurse -Force -ErrorAction SilentlyContinue } }
  }
  if ($cleanupLog) { Remove-Item $cleanupLog -Force -ErrorAction SilentlyContinue }
  if ($runRoot) { Remove-Item $runRoot -Recurse -Force -ErrorAction SilentlyContinue }
  if ($status -ne 0 -or $cleanupStatus -ne 0 -or -not $script:redacted) { exit 1 }
}

try {
  # desktop-ci collects %TEMP%\dci-artifacts on Windows and DCI_ARTIFACTS_DIR is
  # not injected by the nightly, so any other default silently sends the lane
  # down the driver's DCI-NO-ARTIFACTS path with no report at all.
  $artifacts = if ($env:DCI_ARTIFACTS_DIR) { $env:DCI_ARTIFACTS_DIR } else { Join-Path $env:TEMP "dci-artifacts" }
  $runRoot = Join-Path $env:TEMP ("muniment-e2e-" + [guid]::NewGuid().ToString("N"))
  $raw = Join-Path $runRoot "raw"
  $safe = Join-Path $runRoot "safe"
  $stateRoot = Join-Path $runRoot "state"
  $msi = Join-Path $runRoot "muniment-nightly.msi"
  $authUrlFile = Join-Path $runRoot "auth-url"
  $cleanupLog = Join-Path $runRoot "cleanup.log"
  $installerLog = Join-Path $raw "installer.log"
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_SETUP_FAIL -eq "before-directories") { throw "injected setup failure" }
  New-Item -ItemType Directory -Force $raw, $stateRoot | Out-Null
  New-Item -ItemType File -Force $cleanupLog | Out-Null
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1") {
    $installDirectory = Join-Path $runRoot "installed"
    $testRegistration = Join-Path $runRoot "registration"
    $testProcess = Join-Path $runRoot "process"
    New-Item -ItemType Directory -Force $installDirectory | Out-Null
    $testDisplayName = if ($env:MUNIMENT_E2E_FINALIZER_TEST_REGISTRATION_NAME) { $env:MUNIMENT_E2E_FINALIZER_TEST_REGISTRATION_NAME } else { "muniment" }
    Set-Content -LiteralPath $testRegistration -Value $testDisplayName -NoNewline
    if ($env:MUNIMENT_E2E_FINALIZER_TEST_REMAIN_PROCESS -eq "1") { New-Item -ItemType File -Force $testProcess | Out-Null }
    $ready = $true
    $installAttempted = $true
    return
  }

  $imageFixture = Join-Path $stateRoot "image-token.png"
  $imageBase64 = (Get-Content -LiteralPath "test/e2e/fixtures/image-token.png.base64" -Raw) -replace '\s', ''
  [IO.File]::WriteAllBytes($imageFixture, [Convert]::FromBase64String($imageBase64))

  & npm.cmd ci --no-audit --no-fund *>> $installerLog
  if ($LASTEXITCODE -ne 0) { throw "npm dependency installation failed" }
  & npm.cmd test *>> $installerLog
  if ($LASTEXITCODE -ne 0) { throw "Windows contract tests failed" }

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
  $env:MUNIMENT_E2E_HOME_PATH = Join-Path $stateRoot 'home-override'
  $env:MUNIMENT_E2E_IMAGE_PATH = $imageFixture
  $env:MUNIMENT_E2E_FORCE_MANUAL = '1'
  $ready = $true
  $wdioLog = Join-Path $raw "wdio.log"
  $driverAppLog = Join-Path $raw "driver-app.log"
  & npm.cmd run test:e2e 1> $wdioLog 2> $driverAppLog
  if ($LASTEXITCODE -ne 0) { $status = 1 }
} catch {
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_SETUP_FAIL) { $script:redacted = $false }
  if ($installerLog) { Add-Content $installerLog "runner failed: $($_.Exception.Message)" -ErrorAction SilentlyContinue }
  $status = 1
} finally {
  Finalize-Run
}
