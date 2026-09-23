param(
  [Parameter(Mandatory = $true)][string]$AppBinary,
  [Parameter(Mandatory = $true)][string]$AppLibrary,
  [Parameter(Mandatory = $true)][string]$Diagnostics,
  [ValidateRange(0, 120)][int]$WaitSeconds = 60
)
$ErrorActionPreference = 'Stop'
$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
  throw 'The installed smoke requires a non-elevated console session.'
}
New-Item -ItemType Directory -Force $Diagnostics | Out-Null
$beforeExe = (Get-FileHash -LiteralPath $AppBinary -Algorithm SHA256).Hash
$beforeDll = (Get-FileHash -LiteralPath $AppLibrary -Algorithm SHA256).Hash
$stdout = Join-Path $Diagnostics 'installed-app.stdout.log'
$stderr = Join-Path $Diagnostics 'installed-app.stderr.log'
$app = $null
try {
  $app = Start-Process -FilePath $AppBinary -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
  # Hold the process handle so an early exit preserves its exit code.
  $null = $app.Handle
  $deadline = (Get-Date).AddSeconds($WaitSeconds)
  $ready = $false
  do {
    $app.Refresh()
    if ($app.HasExited) { throw "The installed app exited before its first connected window. Exit code: $($app.ExitCode)" }
    $connected = @(Get-Content -LiteralPath $stderr -ErrorAction SilentlyContinue | Where-Object { $_ -match '^desktop runtime client connected=' }) | Select-Object -Last 1
    if ($app.MainWindowHandle -ne [IntPtr]::Zero -and $connected -eq 'desktop runtime client connected=true') {
      $ready = $true
      break
    }
    Start-Sleep -Milliseconds 250
  } while ((Get-Date) -lt $deadline)
  if (-not $ready) { throw 'The installed app did not open a connected window.' }

} finally {
  if ($app -and -not $app.HasExited) {
    # Only this smoke process tree is stopped. The installer owns service cleanup.
    & taskkill.exe /PID $app.Id /T /F | Out-Null
    if (-not $app.WaitForExit(10000)) { throw 'The installed smoke app did not stop.' }
  }
  if ((Get-FileHash -LiteralPath $AppBinary -Algorithm SHA256).Hash -ne $beforeExe -or
      (Get-FileHash -LiteralPath $AppLibrary -Algorithm SHA256).Hash -ne $beforeDll) {
    throw 'The installed smoke changed the shipped application.'
  }
}

  @{
    application_alive = $true
    window_present = $true
    client_connected = $true
    executable_sha256 = $beforeExe
    application_library_sha256 = $beforeDll
    application_modified = $false
  } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Diagnostics 'installed-app-smoke.json')
