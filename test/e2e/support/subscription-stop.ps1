param(
    [Parameter(Mandatory = $true)][string]$LaunchFile
)
$ErrorActionPreference = 'Stop'
Add-Type -Path (Join-Path $PSScriptRoot 'subscription-job.cs')
$launch = Get-Content -LiteralPath $LaunchFile -Raw | ConvertFrom-Json
$job = $null
try {
    $job = [SubscriptionJob]::new($launch.executable, [string[]]$launch.args, $launch.jobName)
    [IO.File]::WriteAllText("$LaunchFile.ready.tmp", [string]$job.Pid)
    [IO.File]::Move("$LaunchFile.ready.tmp", "$LaunchFile.ready")
    # Retain the original root handle and the job even after the root exits.
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while (-not [IO.File]::Exists("$LaunchFile.stop")) {
        if ($clock.Elapsed.TotalMinutes -ge 30) { throw 'The native probe job exceeded its lifetime limit.' }
        if ($job.Exited -and -not [IO.File]::Exists("$LaunchFile.exited")) {
            [IO.File]::WriteAllText("$LaunchFile.exited", '1')
        }
        Start-Sleep -Milliseconds 25
    }
} finally {
    if ($job) {
        try { $job.Stop(15000) } finally { $job.Dispose() }
    }
}
