$ErrorActionPreference = 'Stop'
$env:MUNIMENT_PI_CANDIDATE = '1'
. $PSScriptRoot/prepare-cef-windows.ps1
function Checked([scriptblock]$Command) { & $Command; if ($LASTEXITCODE -ne 0) { throw "Command failed: $LASTEXITCODE" } }
Checked { npm.cmd ci }
Checked { cargo build --manifest-path src-tauri/Cargo.toml --package muniment-runtime --release --locked }
Checked { powershell.exe -NoProfile -ExecutionPolicy Bypass -File .github/build-reader.ps1 src-tauri/target/release/muniment-reader.exe }
Checked { npm.cmd run tauri build -- --no-bundle --features cef-smoke }
Checked { cargo build --manifest-path src-tauri/Cargo.toml --package muniment-desktop --lib --release --locked --features cef-smoke,tauri/custom-protocol }
Checked { node scripts/package-cef-windows.mjs }
$bundle = (Resolve-Path 'src-tauri/target/release/cef-app').Path
$env:MUNIMENT_STATE_DIR = Join-Path $env:TEMP ('muniment-cef-test-' + [guid]::NewGuid().ToString('N'))
$artifacts = if ($env:DCI_ARTIFACTS_DIR) {$env:DCI_ARTIFACTS_DIR} else {Join-Path $env:TEMP 'dci-artifacts'}
New-Item -ItemType Directory -Force $artifacts | Out-Null
foreach ($phase in @('write','read')) {
  $env:MUNIMENT_CEF_SMOKE_PHASE = $phase
  # Chromium can relaunch an elevated caller as a normal user. Keep the test
  # profile in arguments and wait for the relaunched app's shutdown marker.
  $arguments = @('--cef-smoke', "`"--cef-smoke-state=$env:MUNIMENT_STATE_DIR`"", "--cef-smoke-phase=$phase")
  $process = Start-Process -FilePath (Join-Path $bundle 'muniment-desktop.exe') -ArgumentList $arguments -WorkingDirectory $bundle -RedirectStandardOutput (Join-Path $artifacts "cef-$phase.stdout.log") -RedirectStandardError (Join-Path $artifacts "cef-$phase.stderr.log") -PassThru
  $null = $process.Handle
  $closed = Join-Path $env:MUNIMENT_STATE_DIR "browser/smoke-$phase.closed"
  $deadline = (Get-Date).AddSeconds(90)
  while (!(Test-Path $closed) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 250 }
  if (!(Test-Path $closed)) { throw "CEF $phase did not close" }
  $proof = Join-Path $env:MUNIMENT_STATE_DIR "browser/smoke-$phase.json"
  if (!(Test-Path $proof)) { throw "CEF $phase did not produce proof" }
  Copy-Item $proof $artifacts
  $result = Get-Content $proof -Raw | ConvertFrom-Json
  if (!$result.ok) { throw "CEF $phase failed: $($result.error)" }
  $image = Join-Path $env:MUNIMENT_STATE_DIR "browser/smoke-$phase.png.base64"
  [IO.File]::WriteAllBytes((Join-Path $artifacts "cef-$phase.png"),[Convert]::FromBase64String([IO.File]::ReadAllText($image)))
}
Write-Output 'CEF Windows: login, saved profile restart, input, snapshot, screenshot, app isolation, and stop control passed.'
