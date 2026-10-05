param(
    [Parameter(Mandatory = $true)][string]$LaunchFile
)
$ErrorActionPreference = 'Stop'
Add-Type -Path (Join-Path $PSScriptRoot 'subscription-job.cs')
$launch = Get-Content -LiteralPath $LaunchFile -Raw | ConvertFrom-Json
$job = $null
$observed = @{}
function Save-ProcessReceipt($process, [bool]$cleanup) {
    $code = $null
    if ($process.Exited) { $code = $process.ExitCode }
    $receipt = @{ pid = $process.Pid; exit_code = $code; cleanup = $cleanup }
    $file = Join-Path $launch.profile "subscription-probe-process-$($process.Pid).json"
    [IO.File]::WriteAllText($file, (($receipt | ConvertTo-Json -Compress) + "`n"))
}
try {
    $job = [SubscriptionJob]::new($launch.executable, [string[]]$launch.args, $launch.jobName)
    [IO.File]::WriteAllText("$LaunchFile.ready.tmp", [string]$job.Pid)
    [IO.File]::Move("$LaunchFile.ready.tmp", "$LaunchFile.ready")
    if ($launch.profile) { Save-ProcessReceipt $job $false }
    # Retain the original root handle and the job even after the root exits.
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not [IO.File]::Exists("$LaunchFile.stop")) {
        if ($clock.Elapsed.TotalMinutes -ge 30) { throw 'The native probe job exceeded its lifetime limit.' }
        if ($job.Exited -and -not [IO.File]::Exists("$LaunchFile.exited")) {
            [IO.File]::WriteAllText("$LaunchFile.exited", [string]$job.ExitCode)
            if ($launch.profile) { Save-ProcessReceipt $job $false }
        }
        if ($launch.profile) {
            foreach ($start in [IO.Directory]::GetFiles($launch.profile, 'subscription-probe-start-*')) {
                $name = [IO.Path]::GetFileName($start)
                if ($name -cnotmatch '^subscription-probe-start-([1-9][0-9]{0,9})$') { continue }
                $identity = $Matches[1]
                $processId = 0
                if ([int]::TryParse($identity, [ref]$processId) -and -not $observed.ContainsKey($processId)) {
                    if ([IO.File]::ReadAllText($start) -cne $identity) { continue }
                    try {
                        $process = [SubscriptionProcess]::new($processId, $launch.executable)
                        $observed[$processId] = $process
                        Save-ProcessReceipt $process $false
                        [IO.File]::WriteAllText((Join-Path $launch.profile "subscription-probe-observed-$processId"), $identity)
                    } catch {
                        [IO.File]::WriteAllText((Join-Path $launch.profile 'subscription-probe-observer-error'), "process-observe-failed`n")
                        throw 'The probe process observer failed.'
                    }
                }
            }
            foreach ($process in $observed.Values) {
                if ($process.Exited) { Save-ProcessReceipt $process $false }
            }
        }
        Start-Sleep -Milliseconds 25
    }
} finally {
    # Preserve natural exit codes before cleanup stops the job or an unadmitted app.
    $cleanupPids = @{}
    $rootCleanup = $job -and -not $job.Exited
    foreach ($process in $observed.Values) { $cleanupPids[$process.Pid] = -not $process.Exited }
    try {
        if ($job -and $launch.profile) { Save-ProcessReceipt $job $rootCleanup }
        foreach ($process in $observed.Values) { Save-ProcessReceipt $process ($cleanupPids[$process.Pid]) }
    } finally {
        try {
            if ($job) {
                try {
                    $job.Stop(15000)
                    if ($launch.profile) { Save-ProcessReceipt $job $rootCleanup }
                } finally { $job.Dispose() }
            }
        } finally {
            $failed = $false
            foreach ($process in $observed.Values) {
                try {
                    $process.Stop()
                    Save-ProcessReceipt $process ($cleanupPids[$process.Pid])
                } catch { $failed = $true }
                finally { $process.Dispose() }
            }
            if ($failed) { throw 'The observed probe process cleanup failed.' }
        }
    }
}
