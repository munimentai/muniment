param()

try {
  $artifacts = if ($env:DCI_ARTIFACTS_DIR) { $env:DCI_ARTIFACTS_DIR } else { Join-Path $env:TEMP "dci-artifacts" }
  New-Item -ItemType Directory -Force $artifacts -ErrorAction Stop | Out-Null
  # New-Item -Force can accept a file without creating a directory.
  if (-not (Test-Path -LiteralPath $artifacts -PathType Container)) {
    throw "The artifact path is not a directory: $artifacts"
  }
  $diagnosticFile = Join-Path $artifacts "runner-failure.txt"
  $transcriptPath = Join-Path $env:TEMP "dci-windows-transcript.log"
  if ($env:MUNIMENT_E2E_BOOTSTRAP_TEST_FAIL -eq "start-transcript") { throw "injected Start-Transcript failure" }
  Start-Transcript -LiteralPath $transcriptPath -Force -ErrorAction SilentlyContinue | Out-Null
} catch {
  $bootstrapDiagnostic = "message: $($_.Exception.Message)`ncategory: $($_.CategoryInfo.Category)`nline: $($_.InvocationInfo.ScriptLineNumber)"
  Write-Output $bootstrapDiagnostic
  if ($diagnosticFile) { Set-Content -LiteralPath $diagnosticFile -Value $bootstrapDiagnostic -ErrorAction SilentlyContinue }
  exit 1
}
$diagnostic = $null

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$ProgressPreference = "SilentlyContinue"

$runRoot = $null
$raw = $null
$safe = $null
$stateRoot = $null
$msi = $null
$authUrlFile = $null
$cleanupLog = $null
$cleanupStatusLedger = $null
$redactionReport = $null
$installerLog = $null
$status = 0
$cleanupStatus = 0
$script:redacted = $true
$ready = $false
$installAttempted = $false
$installDirectory = $null
$productCode = $null
$installSid = $null
$installLocalAppData = $env:LOCALAPPDATA
. (Join-Path $PSScriptRoot "../../windows-msi-registration.ps1")
$handlerKey = "HKCU:\Software\Classes\muniment-e2e-https"
$httpsKey = "HKCU:\Software\Classes\https"
$testRegistration = $null
$testProcess = $null
$redactor = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../support/redact.mjs"))

function Save-RunnerFailure([System.Management.Automation.ErrorRecord]$Failure) {
  $message = "message: $($Failure.Exception.Message)`ncategory: $($Failure.CategoryInfo.Category)`nline: $($Failure.InvocationInfo.ScriptLineNumber)"
  $script:diagnostic = if ($diagnostic) { "$diagnostic`n$message" } else { $message }
  if ($raw -and (Test-Path -LiteralPath $raw)) {
    Set-Content -LiteralPath (Join-Path $raw "runner-failure.txt") -Value $diagnostic -Encoding UTF8 -ErrorAction SilentlyContinue
  }
  Write-Output $message
  $script:status = 1
}

function Invoke-BoundedProcess([string]$File, [string]$Arguments, [int]$TimeoutSeconds, [string]$Log) {
  $errorLog = $Log + ".err"
  $process = Start-Process $File -ArgumentList $Arguments -PassThru -RedirectStandardOutput $Log -RedirectStandardError $errorLog
  try {
    # Cache the handle before waiting so Windows PowerShell can read ExitCode.
    $null = $process.Handle
    $timedOut = -not $process.WaitForExit($TimeoutSeconds * 1000)
    if ($timedOut) {
      Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    }
    $stopped = $process.WaitForExit(10000)
    if (Test-Path -LiteralPath $errorLog) {
      Get-Content -LiteralPath $errorLog | Add-Content -LiteralPath $Log
      Remove-Item -LiteralPath $errorLog -Force
    }
    if (-not $stopped) { throw "$File timed out" }
    $exitCode = $process.ExitCode
    if ($null -eq $exitCode) { throw "$File did not report an exit code" }
    Add-Content -LiteralPath $Log -Value "$File exited with code $exitCode"
    if ($timedOut) { throw "$File timed out" }
    if ($exitCode -notin @(0, 3010)) { throw "$File failed with exit code $exitCode" }
  } finally {
    $process.Dispose()
  }
}

function Resolve-NativeCommand([string]$File, [string]$FailureMessage) {
  try {
    return (Get-Command $File -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
  } catch {
    throw "$FailureMessage`: could not resolve $File`: $($_.Exception.Message)"
  }
}

function Invoke-NativeCommand([string]$File, [string]$Arguments, [string]$Log, [string]$FailureMessage, [string]$InputText = $null, [string]$ErrorLog = $null, [bool]$SummarizeFailure = $true) {
  $resolvedFile = Resolve-NativeCommand $File $FailureMessage
  $startInfo = New-Object Diagnostics.ProcessStartInfo
  if ([IO.Path]::GetExtension($resolvedFile) -eq ".cmd") {
    $startInfo.FileName = $env:ComSpec
    $startInfo.Arguments = "/d /s /c `"`"$resolvedFile`" $Arguments`""
  } else {
    $startInfo.FileName = $resolvedFile
    $startInfo.Arguments = $Arguments
  }
  # PowerShell location and the process current directory can differ.
  $startInfo.WorkingDirectory = (Get-Location).ProviderPath
  $startInfo.UseShellExecute = $false
  $startInfo.CreateNoWindow = $true
  $startInfo.RedirectStandardOutput = $true
  $startInfo.RedirectStandardError = $true
  $startInfo.RedirectStandardInput = $null -ne $InputText
  $process = New-Object Diagnostics.Process
  $process.StartInfo = $startInfo
  try {
    if (-not $process.Start()) { throw "the process did not start" }
  } catch {
    throw "$FailureMessage`: could not start $File`: $($_.Exception.Message)"
  }
  $stdoutTask = $process.StandardOutput.ReadToEndAsync()
  $stderrTask = $process.StandardError.ReadToEndAsync()
  if ($null -ne $InputText) {
    $process.StandardInput.Write($InputText)
    $process.StandardInput.Close()
  }
  $process.WaitForExit()
  $stdout = $stdoutTask.Result
  $stderr = $stderrTask.Result
  if ($stdout -and $Log) { Add-Content -LiteralPath $Log -Value $stdout -NoNewline }
  $stderrLog = if ($ErrorLog) { $ErrorLog } else { $Log }
  if ($stderr -and $stderrLog) { Add-Content -LiteralPath $stderrLog -Value $stderr -NoNewline }
  if ($process.ExitCode -ne 0) {
    if (-not $SummarizeFailure) { throw "diagnostic summary unavailable" }
    # Share redaction and diagnostic selection with the POSIX runner.
    $summaryHelper = Join-Path $PSScriptRoot "../support/failure-summary.mjs"
    $summaryInput = @{ stdout = $stdout; stderr = $stderr; label = $FailureMessage; exitCode = $process.ExitCode } | ConvertTo-Json -Compress
    try {
      # Keep the summary's progress excerpts out of the command log.
      $detail = Invoke-NativeCommand "node" "`"$summaryHelper`"" $null "diagnostic summary failed" $summaryInput $null $false
    } catch {
      throw "native command failed (exit code $($process.ExitCode)), diagnostic summary unavailable"
    }
    throw $detail
  }
  return $stdout
}

function Write-ToolchainState([string]$Phase) {
  Write-Output "Tauri toolchain $Phase process_cwd=$([Environment]::CurrentDirectory) command_cwd=$((Get-Location).ProviderPath)"
  Write-Output "Tauri toolchain $Phase npm=$(Resolve-NativeCommand 'npm.cmd' 'toolchain probe failed')"
  # --call uses Node from PATH without asking npm to resolve the node package.
  # Relative paths avoid nested cmd.exe quotes when the repository path contains spaces.
  Invoke-NativeCommand "npm.cmd" "exec --offline --call `"node test/e2e/support/windows-toolchain.mjs . $Phase`"" $installerLog "Tauri toolchain probe failed"
}

function Get-UninstallEntries([ValidateSet("HKCU", "HKLM")][string]$Hive = "HKCU") {
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1") {
    if ($env:MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE) {
      if (Test-Path -LiteralPath $env:MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE) {
        $entries = Get-Content -LiteralPath $env:MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE -Raw | ConvertFrom-Json
        return @($entries | Where-Object { $_.Hive -eq $Hive })
      }
      return @()
    }
    if ($Hive -eq "HKCU" -and $testRegistration -and (Test-Path -LiteralPath $testRegistration)) {
      return @([pscustomobject]@{ PSChildName = "test-product"; PSPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\test-product"; DisplayName = (Get-Content -LiteralPath $testRegistration -Raw) })
    }
    return @()
  }
  $roots = @(
    "$Hive`:\Software\Microsoft\Windows\CurrentVersion\Uninstall",
    "$Hive`:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"
  )
  return @($roots | Where-Object { Test-Path $_ } | ForEach-Object { Get-ChildItem $_ | ForEach-Object {
    $properties = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
    if ($properties) {
      $displayName = $properties.PSObject.Properties["DisplayName"]
      [pscustomobject]@{ PSChildName = $_.PSChildName; PSPath = $_.PSPath; DisplayName = $(if ($displayName) { $displayName.Value } else { $null }) }
    }
  } })
}

function Get-ProductRegistration([object[]]$Entries = @(Get-UninstallEntries)) {
  return @($Entries | Where-Object { $_.DisplayName -eq "muniment" })
}

function Save-RegistrationSnapshot([string]$Phase) {
  $userEntries = @(Get-UninstallEntries "HKCU")
  $machineEntries = @(Get-UninstallEntries "HKLM")
  $userRegistrations = @(Get-ProductRegistration $userEntries)
  $machineRegistrations = @(Get-ProductRegistration $machineEntries)
  $snapshot = [ordered]@{
    hkcu = @{ count = $userRegistrations.Count; entries = $userRegistrations }
    hklm = @{ count = $machineRegistrations.Count; entries = $machineRegistrations }
    userUninstallEntries = $userEntries
    machineUninstallEntries = $machineEntries
    scope = $(if ($productCode) { Get-PerUserMsiRegistration $productCode $installSid } else { $null })
  }
  ConvertTo-Json -InputObject $snapshot -Depth 5 | Set-Content -LiteralPath (Join-Path $raw "registration-$Phase.json") -Encoding UTF8
  return $snapshot
}

function Assert-ProductRegistration($Registration) {
  try {
    Assert-PerUserMsiRegistration $Registration $installLocalAppData
  } catch {
    $registrationFailure = $_
    # Publish redacted evidence before the assertion throws.
    try {
      Invoke-NativeCommand "node" "`"$redactor`" `"$raw`" `"$safe`"" $cleanupLog "registration artifact redaction failed" | Out-Null
      Get-ChildItem -LiteralPath $safe -File | Copy-Item -Destination $artifacts -Force
    } catch {
      Write-Output "Registration artifact publication failed."
    }
    throw $registrationFailure
  }
}

function Install-Product {
  $script:productCode = Get-MsiProductCode $msi
  $script:installSid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
  Save-RegistrationSnapshot "before" | Out-Null
  $msiLog = Join-Path $runRoot "msi-verbose.log"
  $script:installAttempted = $true
  try {
    Invoke-BoundedProcess "msiexec.exe" "/i `"$msi`" /qn /norestart /L*V `"$msiLog`"" 180 (Join-Path $raw "installer-process.log")
  } finally {
    # Decode the MSI log before the UTF-8 redactor reads it.
    if (Test-Path -LiteralPath $msiLog) {
      $msiText = Get-Content -LiteralPath $msiLog -Raw
      $msiText | Set-Content -LiteralPath (Join-Path $raw "msi-verbose.log") -Encoding UTF8
      $msiText | Add-Content -LiteralPath $installerLog -Encoding UTF8
      $elevationLines = @($msiText -split '\r?\n' | Where-Object { $_ -match '\bMsiRunningElevated\b' })
      if ($elevationLines.Count -eq 0) { Write-Host "The per-user MSI log has no MsiRunningElevated lines." }
      $elevationLines | ForEach-Object { Write-Host $_ }
    }
    $snapshot = Save-RegistrationSnapshot "after"
    Write-PerUserMsiRegistration $snapshot.scope "installed"
  }
  Assert-ProductRegistration $snapshot.scope
  $script:installDirectory = $snapshot.scope.InstallLocation
  $script:installDisplayIcon = $snapshot.scope.DisplayIcon
}

function Get-HarnessProcesses {
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1") {
    if ($testProcess -and (Test-Path -LiteralPath $testProcess)) { return @([pscustomobject]@{ ProcessName = "muniment" }) }
    return @()
  }
  $automation = @(Get-CimInstance Win32_Process | Where-Object {
    $_.CommandLine -like '*wdio*test/e2e/wdio.conf.js*' -or $_.CommandLine -like '*@wdio*local-runner*run.js*'
  } | ForEach-Object { Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue })
  return @(Get-Process muniment, muniment-desktop, muniment-runtime, msedgedriver -ErrorAction SilentlyContinue) + $automation
}

function Stop-HarnessProcesses {
  # Stop the runtime task so its restart policy cannot race the next spec.
  $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
  $task = Get-ScheduledTask -TaskPath "\Muniment\" -TaskName "Runtime-$sid" -ErrorAction SilentlyContinue
  if ($task) { Stop-ScheduledTask -InputObject $task }
  $deadline = [DateTime]::UtcNow.AddSeconds(10)
  do {
    $processes = @(Get-HarnessProcesses)
    $task = Get-ScheduledTask -TaskPath "\Muniment\" -TaskName "Runtime-$sid" -ErrorAction SilentlyContinue
    if ($processes.Count -eq 0 -and (-not $task -or $task.State -ne 'Running')) { return }
    $processes | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 100
  } while ([DateTime]::UtcNow -lt $deadline)
  throw "Spec processes did not stop."
}

function Invoke-E2e([string]$Log, [string]$FailureMessage, [string]$Spec = '') {
  Invoke-Cleanup "stop-before-spec" { Stop-HarnessProcesses }
  if ($script:cleanupLastStatus -ne 0) { throw "Spec process cleanup failed. The runner did not start the next spec." }
  Add-Content $cleanupLog "start-spec: $([IO.Path]::GetFileName($Log))"
  $arguments = "run test:e2e"
  if ($Spec) { $arguments += " -- --spec $Spec" }
  Invoke-NativeCommand "npm.cmd" $arguments $Log $FailureMessage $null (Join-Path $raw "driver-app.log")
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
    $script:cleanupLastStatus = $phaseStatus
    if ($cleanupStatusLedger) {
      Add-Content $cleanupStatusLedger "$Name`: $(if ($phaseStatus) { 'failed' } else { 'ok' })" -ErrorAction SilentlyContinue
    }
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
  if ($ready) {
    Invoke-Cleanup "revoke-session" {
      Stop-HarnessProcesses
      Write-Output 'stop-before-spec: ok'
      $env:MUNIMENT_E2E_CLEANUP_ONLY = "1"
      Invoke-BoundedProcess "npm.cmd" "run test:e2e" 45 (Join-Path $raw "cleanup-wdio.log")
    }
  }
  Invoke-Cleanup "stop-app" { Stop-HarnessProcesses }
  if ($installAttempted) {
    Invoke-Cleanup "uninstall" {
      if ($env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1") {
        if ($env:MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE) { Remove-Item -LiteralPath $env:MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE -Force }
        if ($env:MUNIMENT_E2E_FINALIZER_TEST_REMAIN_REGISTRATION -ne "1" -and $testRegistration) { Remove-Item $testRegistration -Force }
        if ($env:MUNIMENT_E2E_FINALIZER_TEST_REMAIN_FILES -ne "1" -and $installDirectory) { Remove-Item $installDirectory -Recurse -Force }
      } elseif ($productCode) { Invoke-BoundedProcess "msiexec.exe" "/x $productCode /qn /norestart" 180 (Join-Path $raw "uninstaller.log") }
    }
  }
  $uninstalledRegistration = @{ scope = $null }
  Invoke-Cleanup "registration-gone" {
    if ($productCode) {
      $snapshot = Save-RegistrationSnapshot "uninstalled"
      $uninstalledRegistration.scope = $snapshot.scope
      Write-PerUserMsiRegistration $snapshot.scope "uninstalled"
      Assert-PerUserMsiRegistration $snapshot.scope $installLocalAppData -Absent
    } elseif (@(Get-ProductRegistration).Count -ne 0) { throw "product registration remains" }
  }
  # Write directly to the host outside the cleanup wrapper's stream redirection.
  if ($uninstalledRegistration.scope) {
    Write-PerUserMsiRegistration $uninstalledRegistration.scope "uninstalled"
  }
  Invoke-Cleanup "installed-files-gone" { if ($installDirectory -and (Test-Path -LiteralPath $installDirectory)) { throw "installed files remain" } }
  Invoke-Cleanup "remove-auth-handler" { Remove-AuthHandler }
  Invoke-Cleanup "remove-state" { if ($stateRoot) { Remove-Item $stateRoot -Recurse -Force -ErrorAction SilentlyContinue } }
  Invoke-Cleanup "processes-gone" {
    if (@(Get-HarnessProcesses).Count -ne 0) { throw "test process remains" }
  }
  # Keep the tail even if artifact publication fails after Stop-Transcript.
  if ($installerLog -and (Test-Path -LiteralPath $installerLog)) {
    try {
      $tailHelper = Join-Path $PSScriptRoot "../support/installer-log-tail.mjs"
      Invoke-NativeCommand "node" "`"$tailHelper`" `"$installerLog`"" $cleanupLog "installer log tail failed" | ForEach-Object { Write-Host $_ }
    } catch {
      Write-Output "The installer.log tail is unavailable."
    }
  }
  Stop-Transcript -ErrorAction SilentlyContinue | Out-Null
  if ($raw -and (Test-Path -LiteralPath $raw)) {
    Copy-Item -LiteralPath $transcriptPath -Destination (Join-Path $raw "runner-transcript.log") -Force -ErrorAction SilentlyContinue
  }
  if ($cleanupLog -and $raw) { Copy-Item $cleanupLog (Join-Path $raw "cleanup.log") -Force -ErrorAction SilentlyContinue }
  Invoke-Cleanup "index-failure-artifacts" {
    Get-ChildItem -LiteralPath $raw -File |
      Where-Object { $_.Name -like "page-source-*.html" -or $_.Name -like "screenshot-*.png" } |
      Select-Object -ExpandProperty FullName |
      Set-Content (Join-Path $raw "failure-artifacts.log")
  }
  Invoke-Cleanup "redact-artifacts" {
    if (-not $raw -or -not (Test-Path $raw)) { throw "raw staging is unavailable" }
    try {
      Invoke-NativeCommand "node" "`"$redactor`" `"$raw`" `"$safe`" `"$redactionReport`"" $cleanupLog "artifact redaction failed"
    } catch {
      $script:redacted = $false
      throw
    }
  }
  Invoke-Cleanup "remove-raw" { if ($raw) { Remove-Item $raw -Recurse -Force -ErrorAction SilentlyContinue } }
  Invoke-Cleanup "remove-msi" { if ($msi) { Remove-Item $msi -Force -ErrorAction SilentlyContinue } }
  Invoke-Cleanup "remove-auth-url" { if ($authUrlFile) { Remove-Item $authUrlFile -Force -ErrorAction SilentlyContinue } }
  if ($script:redacted) {
    Invoke-Cleanup "publish-artifacts" { if (-not $safe -or -not (Test-Path $safe)) { throw "safe staging is unavailable" }; Remove-Item $artifacts -Recurse -Force -ErrorAction SilentlyContinue; Move-Item $safe $artifacts }
    if ($script:cleanupLastStatus -ne 0) { Invoke-Cleanup "suppress-artifacts" { Remove-Item $artifacts -Recurse -Force -ErrorAction SilentlyContinue; if ($safe) { Remove-Item $safe -Recurse -Force -ErrorAction SilentlyContinue } } }
  } else {
    Invoke-Cleanup "suppress-artifacts" { if ($artifacts) { Remove-Item $artifacts -Recurse -Force -ErrorAction SilentlyContinue }; if ($safe) { Remove-Item $safe -Recurse -Force -ErrorAction SilentlyContinue } }
    try {
      New-Item -ItemType Directory -Force $artifacts | Out-Null
      Set-Content (Join-Path $artifacts "envelope-reason.txt") "envelope: minimal`nwithheld: guest artifacts`nreason: redaction-failed"
      if ($cleanupStatusLedger -and (Test-Path $cleanupStatusLedger)) {
        Copy-Item $cleanupStatusLedger (Join-Path $artifacts "cleanup-status.log")
      } else {
        New-Item -ItemType File -Force (Join-Path $artifacts "cleanup-status.log") | Out-Null
      }
      if ($redactionReport -and (Test-Path $redactionReport) -and (Get-Item $redactionReport).Length -gt 0) {
        Copy-Item $redactionReport (Join-Path $artifacts "redaction-failure.txt")
      } else {
        Set-Content (Join-Path $artifacts "redaction-failure.txt") "file: unknown`ncategory: redactor-process"
      }
    } catch {
      $script:cleanupStatus = 1
    }
  }
  if ($cleanupLog) { Remove-Item $cleanupLog -Force -ErrorAction SilentlyContinue }
  if ($runRoot) { Remove-Item $runRoot -Recurse -Force -ErrorAction SilentlyContinue }
  if ($cleanupStatus -ne 0 -or -not $script:redacted) { $script:status = 1 }
}

try {
  # desktop-ci collects %TEMP%\dci-artifacts on Windows and DCI_ARTIFACTS_DIR is
  # not injected by the nightly, so any other default silently sends the lane
  # down the driver's DCI-NO-ARTIFACTS path with no report at all.
  $runRoot = Join-Path $env:TEMP ("muniment-e2e-" + [guid]::NewGuid().ToString("N"))
  $raw = Join-Path $runRoot "raw"
  $safe = Join-Path $runRoot "safe"
  $stateRoot = Join-Path $runRoot "state"
  $msi = Join-Path $runRoot "muniment-nightly.msi"
  $authUrlFile = Join-Path $runRoot "auth-url"
  $cleanupLog = Join-Path $runRoot "cleanup.log"
  $cleanupStatusLedger = Join-Path $runRoot "cleanup-status.log"
  $redactionReport = Join-Path $runRoot "redaction-failure.txt"
  $installerLog = Join-Path $raw "installer.log"
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_SETUP_FAIL -eq "before-directories") { throw "injected setup failure" }
  New-Item -ItemType Directory -Force $raw, $stateRoot | Out-Null
  New-Item -ItemType File -Force $cleanupLog | Out-Null
  if ($env:MUNIMENT_E2E_RUNNER_TEST_ERROR) { throw $env:MUNIMENT_E2E_RUNNER_TEST_ERROR }
  if ($env:MUNIMENT_E2E_BOUNDED_PROCESS_TEST_EXIT_CODE) {
    $boundedTestExitCode = [int]$env:MUNIMENT_E2E_BOUNDED_PROCESS_TEST_EXIT_CODE
    Invoke-BoundedProcess "cmd.exe" "/d /c `"echo bounded stdout & echo bounded stderr 1>&2 & exit /b $boundedTestExitCode`"" 10 $installerLog
    return
  }
  if ($env:MUNIMENT_E2E_NATIVE_COMMAND_TEST_SCRIPT) {
    Invoke-NativeCommand "node" "`"$($env:MUNIMENT_E2E_NATIVE_COMMAND_TEST_SCRIPT)`"" $installerLog "native command test failed"
    return
  }
  if ($env:MUNIMENT_E2E_NATIVE_COMMAND_TEST_EXIT_CODE) {
    $nativeTestExitCode = [int]$env:MUNIMENT_E2E_NATIVE_COMMAND_TEST_EXIT_CODE
    $nativeTestOutput = switch ($env:MUNIMENT_E2E_NATIVE_COMMAND_TEST_OUTPUT) {
      "stdout" { "echo fatal: installer rejected package signature" }
      "both" { "echo fatal: installer rejected package signature & echo native warning 1>&2" }
      "long-stdout" { "echo $('p' * 1800) & echo fatal: installer rejected package signature 1>&2" }
      "long-stderr" { "echo fatal: installer rejected package signature & echo $('p' * 1800) 1>&2" }
      default { "echo native warning 1>&2" }
    }
    Invoke-NativeCommand "cmd.exe" "/d /c `"$nativeTestOutput & exit /b $nativeTestExitCode`"" $installerLog "native command test failed"
    return
  }
  if ($env:MUNIMENT_E2E_NATIVE_COMMAND_TEST_INVOCATION_ERROR -eq "1") {
    Invoke-NativeCommand "muniment-command-that-does-not-exist" "" $installerLog "native command test failed"
    return
  }
  if ($env:MUNIMENT_E2E_NATIVE_COMMAND_TEST_RESOLUTION -eq "1") {
    $resolvedNpm = Resolve-NativeCommand "npm.cmd" "native command test failed"
    Add-Content -LiteralPath $installerLog -Value $resolvedNpm
    return
  }
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1" -and $env:MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE) {
    $installLocalAppData = Split-Path -Parent $env:MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE
    function Get-MsiProductCode { return '{12345678-1234-ABCD-EF12-34567890ABCD}' }
    function Get-PerUserMsiRegistration {
      $fixture = $env:MUNIMENT_E2E_REGISTRATION_TEST_FIXTURE
      $scope = Get-Content -LiteralPath "$fixture.scope.json" -Raw | ConvertFrom-Json
      $keys = [ordered]@{}
      foreach ($property in $scope.keys.PSObject.Properties) {
        $keys[$property.Name] = @{ path = $property.Value.path; present = $property.Value.present }
      }
      $uninstall = [ordered]@{}
      foreach ($property in $scope.uninstall.PSObject.Properties) { $uninstall[$property.Name] = @($property.Value) }
      if (-not (Test-Path -LiteralPath $fixture)) {
        foreach ($key in $keys.Values) { $key.present = $false }
        foreach ($hive in @($uninstall.Keys)) { $uninstall[$hive] = @() }
        $scope.InstallLocation = ''
      }
      return @{ ProductCode = $scope.ProductCode; Sid = $scope.Sid; keys = $keys; uninstall = $uninstall; InstallLocation = $scope.InstallLocation; DisplayIcon = '' }
    }
    function Invoke-BoundedProcess {
      $scope = Get-PerUserMsiRegistration
      New-Item -ItemType Directory -Force $scope.InstallLocation | Out-Null
      Set-Content -LiteralPath (Join-Path $runRoot "msi-verbose.log") -Value "MSI fixture: caf$([char]0xE9) $env:MUNIMENT_E2E_PASSWORD`nProperty(S): MsiRunningElevated = 1" -Encoding Unicode
    }
    Install-Product
    return
  }
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_MODE -eq "1") {
    if ($env:MUNIMENT_E2E_FINALIZER_TEST_TRANSCRIPT_TEXT) { Write-Output $env:MUNIMENT_E2E_FINALIZER_TEST_TRANSCRIPT_TEXT }
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

  $repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../../.."))
  Set-Location -LiteralPath $repoRoot
  $imageFixture = Join-Path $stateRoot "image-token.png"
  $imageBase64 = (Get-Content -LiteralPath "test/e2e/fixtures/image-token.png.base64" -Raw) -replace '\s', ''
  [IO.File]::WriteAllBytes($imageFixture, [Convert]::FromBase64String($imageBase64))

  Invoke-NativeCommand "npm.cmd" "ci --no-audit --no-fund" $installerLog "npm dependency installation failed"
  Write-ToolchainState "after-npm-ci"
  Invoke-NativeCommand "npx.cmd" "vitest run --root . test/desktop-e2e-harness.test.js" $installerLog "Windows contract tests failed for test/desktop-e2e-harness.test.js"
  Write-ToolchainState "after-contract-tests"

  $sha = $env:MUNIMENT_E2E_SOURCE_SHA
  if ($sha -notmatch '^[0-9a-f]{40}$') { throw "invalid source SHA" }
  if (-not $env:GH_TOKEN -or -not $env:GITHUB_REPOSITORY -or -not $env:MUNIMENT_E2E_USERNAME -or -not $env:MUNIMENT_E2E_PASSWORD) {
    throw "required injected environment is unavailable"
  }

  # Resolve all identity checks before mutating installer or per-user state.
  try {
    $release = (Invoke-WebRequest -UseBasicParsing -Headers @{ Accept = "application/vnd.github+json"; Authorization = "Bearer $($env:GH_TOKEN)" } `
      -Uri "https://api.github.com/repos/$($env:GITHUB_REPOSITORY)/releases/tags/nightly").Content
  } catch {
    throw "nightly release lookup failed: $($_.Exception.Message)"
  }
  Add-Content -LiteralPath $installerLog -Value $release -NoNewline
  $assetId = Invoke-NativeCommand "node" "test/e2e/support/asset-identity.mjs $sha windows" $installerLog "Windows artifact identity validation failed" $release
  if ($assetId -notmatch '^[1-9][0-9]*$') { throw "Windows artifact identity validation failed" }
  Invoke-WebRequest -UseBasicParsing -Headers @{ Accept = "application/octet-stream"; Authorization = "Bearer $($env:GH_TOKEN)" } `
    -Uri "https://api.github.com/repos/$($env:GITHUB_REPOSITORY)/releases/assets/$assetId" -OutFile $msi

  $installer = New-Object -ComObject WindowsInstaller.Installer
  $database = $installer.GetType().InvokeMember("OpenDatabase", "InvokeMethod", $null, $installer, @([string]$msi, [int]0))
  $view = $database.GetType().InvokeMember("OpenView", "InvokeMethod", $null, $database, @([string]"SELECT ``Value`` FROM ``Property`` WHERE ``Property``='ProductName'"))
  $view.GetType().InvokeMember("Execute", "InvokeMethod", $null, $view, $null)
  $record = $view.GetType().InvokeMember("Fetch", "InvokeMethod", $null, $view, $null)
  if ($null -eq $record) { throw "package ProductName is missing" }
  $productName = $record.GetType().InvokeMember("StringData", "GetProperty", $null, $record, @([int]1))
  if ($productName -cne "muniment") { throw "package identity mismatch" }
  Write-Output "MSI ProductName: $productName"

  $env:APPDATA = Join-Path $stateRoot "Roaming"
  $env:LOCALAPPDATA = Join-Path $stateRoot "Local"
  $env:npm_config_cache = Join-Path $runRoot "npm-cache"
  New-Item -ItemType Directory -Force $env:APPDATA, $env:LOCALAPPDATA | Out-Null
  Install-Product
  if (-not $installDirectory) { throw "installer metadata does not identify an install directory" }
  $appBinary = Join-Path $installDirectory "muniment-desktop.exe"
  if (-not (Test-Path -LiteralPath $appBinary -PathType Leaf)) {
    Write-Output "InstallLocation contains these files: $installDirectory"
    if (Test-Path -LiteralPath $installDirectory -PathType Container) {
      Get-ChildItem -LiteralPath $installDirectory -File -Recurse -Force | ForEach-Object { Write-Output $_.FullName }
    } else {
      Write-Output "The InstallLocation directory is missing: $installDirectory"
    }
    throw "The installed desktop executable is missing: $appBinary"
  }
  Write-Output "The installed desktop executable is $appBinary"

  Invoke-NativeCommand "node" "test/e2e/support/webdriver-release-guard.mjs absent `"$appBinary`"" $installerLog "release WebDriver guard failed"
  Write-ToolchainState "before-e2e-build"
  # Use the installed CLI entry without npm's bare-command PATH lookup.
  $tauriCli = Join-Path $repoRoot "node_modules/@tauri-apps/cli/tauri.js"
  if (-not (Test-Path -LiteralPath $tauriCli -PathType Leaf)) { throw "The local Tauri CLI entry is missing: $tauriCli" }
  Invoke-NativeCommand "node" "`"$tauriCli`" build --no-bundle --features e2e-webdriver --config src-tauri/tauri.e2e.conf.json" $installerLog "E2E application build failed"
  $appBinary = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../../../src-tauri/target/release/muniment-desktop.exe"))
  if (-not (Test-Path -LiteralPath $appBinary -PathType Leaf)) { throw "E2E application binary is unavailable" }
  Invoke-NativeCommand "node" "test/e2e/support/webdriver-release-guard.mjs present `"$appBinary`"" $installerLog "E2E WebDriver guard failed"

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
  $env:MUNIMENT_E2E_IMAGE_PATH = $imageFixture
  $ready = $true
  # Run the installed chat specs first. Each later phase still runs after a failure.
  $env:APPDATA = Join-Path $stateRoot "Degraded\Roaming"
  $env:LOCALAPPDATA = Join-Path $stateRoot "Degraded\Local"
  $env:MUNIMENT_E2E_HOME_PATH = Join-Path $stateRoot 'degraded-home'
  try {
    Invoke-E2e (Join-Path $raw "wdio.log") "Windows local-mode tests failed" 'test/e2e/specs/local-mode-chat.spec.js'
  } catch {
    Save-RunnerFailure $_
  }
  try {
    $localModeMarker = Join-Path $env:APPDATA 'ai.muniment.desktop\local-mode'
    if (Test-Path -LiteralPath $localModeMarker) { Remove-Item -LiteralPath $localModeMarker -Force }
    Invoke-E2e (Join-Path $raw "wdio-sign-in.log") "Windows sign-in tests failed" 'test/e2e/specs/real-sign-in.spec.js'
  } catch {
    Save-RunnerFailure $_
  }

  $runtimeConnectionLog = Join-Path $raw "runtime-connection.log"
  $pipePresent = $false
  $waitSeconds = 0
  $runtimeTaskState = "unavailable"
  $runtimeProbeError = $null
  $pipePath = $null
  try {
    $sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    $pipeHelper = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../support/windows-attach-pipe.mjs"))
    $pipePath = (Invoke-NativeCommand "node" "`"$pipeHelper`" $sid" $runtimeConnectionLog "Windows attach pipe derivation failed").Trim()
    $runtimeTask = Get-ScheduledTask -TaskPath "\Muniment\" -TaskName "Runtime-$sid" -ErrorAction SilentlyContinue
    if ($runtimeTask) { $runtimeTaskState = $runtimeTask.State.ToString() }

    Add-Type -TypeDefinition @'
using System.Runtime.InteropServices;

namespace MunimentE2e {
  public static class NamedPipe {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool WaitNamedPipe(string name, uint timeout);
  }
}
'@
    $waitStarted = [DateTime]::UtcNow
    $waitDeadline = $waitStarted.AddSeconds(60)
    do {
      $remainingMilliseconds = [Math]::Ceiling(($waitDeadline - [DateTime]::UtcNow).TotalMilliseconds)
      if ($remainingMilliseconds -le 0) { break }
      $waitMilliseconds = [uint32][Math]::Min(1000, $remainingMilliseconds)
      $pipePresent = [MunimentE2e.NamedPipe]::WaitNamedPipe($pipePath, $waitMilliseconds)
      if ($pipePresent) { break }
      $remainingMilliseconds = [Math]::Ceiling(($waitDeadline - [DateTime]::UtcNow).TotalMilliseconds)
      if ($remainingMilliseconds -le 0) { break }
      Start-Sleep -Milliseconds ([Math]::Min(100, $remainingMilliseconds))
    } while ($true)
    $waitSeconds = [Math]::Min(60, [Math]::Ceiling(([DateTime]::UtcNow - $waitStarted).TotalSeconds))
  } catch {
    $runtimeProbeError = $_.Exception.Message
    Save-RunnerFailure $_
  }

  @(
    "pipe_present=$($pipePresent.ToString().ToLowerInvariant())"
    "wait_seconds=$waitSeconds"
    "task_state=$runtimeTaskState"
  ) | Set-Content -LiteralPath $runtimeConnectionLog
  if ($runtimeProbeError) { Add-Content -LiteralPath $runtimeConnectionLog -Value "error=$runtimeProbeError" }

  if ($pipePresent) {
    try {
      Invoke-NativeCommand "node" "`"test/e2e/support/probe-companion-pairing.mjs`" `"$pipePath`" installed-windows-smoke" (Join-Path $raw "companion-pairing.log") "Windows companion pairing probe failed"
    } catch {
      Save-RunnerFailure $_
    }
  } else {
    $status = 1
  }

  $env:APPDATA = Join-Path $stateRoot "Ready\Roaming"
  $env:LOCALAPPDATA = Join-Path $stateRoot "Ready\Local"
  $env:MUNIMENT_E2E_HOME_PATH = Join-Path $stateRoot 'ready-home'
  $env:MUNIMENT_E2E_ONBOARDING_ONLY = "1"
  try {
    Invoke-E2e (Join-Path $raw "wdio-onboarding.log") "Windows onboarding tests failed"
  } catch {
    Save-RunnerFailure $_
  }
  Remove-Item Env:MUNIMENT_E2E_ONBOARDING_ONLY -ErrorAction SilentlyContinue
} catch {
  Save-RunnerFailure $_
  if ($env:MUNIMENT_E2E_FINALIZER_TEST_SETUP_FAIL) { $script:redacted = $false }
  # A multiline summary must not displace the command's stderr from the log tail.
  if ($installerLog) { Add-Content $installerLog "runner failed: $($_.Exception.Message -replace '\r?\n', ' ')" -ErrorAction SilentlyContinue }
  $status = 1
} finally {
  $diagnosticRoot = Join-Path $env:TEMP ([guid]::NewGuid().ToString("N"))
  $diagnosticRaw = Join-Path $diagnosticRoot "raw"
  try {
    if ($diagnostic -and $raw -and (Test-Path -LiteralPath $raw)) {
      New-Item -ItemType Directory -Force $diagnosticRaw | Out-Null
      foreach ($name in @("installer.log", "msi-verbose.log", "registration-before.json", "registration-after.json")) {
        $source = Join-Path $raw $name
        if (Test-Path -LiteralPath $source) { Copy-Item -LiteralPath $source -Destination $diagnosticRaw }
      }
    }
  } catch {
    Save-RunnerFailure $_
  }
  try {
    Finalize-Run
  } catch {
    Save-RunnerFailure $_
  }
  New-Item -ItemType Directory -Force $artifacts -ErrorAction SilentlyContinue | Out-Null
  if ($diagnostic) {
    # Publish the cause independently so cleanup cannot discard it.
    try {
      $diagnosticSafe = Join-Path $diagnosticRoot "safe"
      New-Item -ItemType Directory -Force $diagnosticRaw | Out-Null
      Set-Content -LiteralPath (Join-Path $diagnosticRaw "runner-failure.txt") -Value $diagnostic -Encoding UTF8
      Invoke-NativeCommand "node" "`"$redactor`" `"$diagnosticRaw`" `"$diagnosticSafe`"" (Join-Path $diagnosticRoot "redaction.log") "runner failure redaction failed" | Out-Null
      Copy-Item -LiteralPath (Join-Path $diagnosticSafe "runner-failure.txt") -Destination $diagnosticFile -Force
      Get-ChildItem -LiteralPath $diagnosticSafe -File | Where-Object Name -NE "runner-failure.txt" | Copy-Item -Destination $artifacts -Force
    } catch {
      $status = 1
    } finally {
      Remove-Item -LiteralPath $diagnosticRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
  }
  if ($status -ne 0) {
    Write-Output "dci: Windows runner transcript tail"
    Get-Content -LiteralPath $transcriptPath -Tail 200 -ErrorAction SilentlyContinue | Write-Output
  }
  Remove-Item -LiteralPath $transcriptPath -Force -ErrorAction SilentlyContinue
  exit $status
}
