param(
    [Parameter(Mandatory = $true)][ValidateRange(1, 2147483647)][int]$RootPid,
    [ValidateRange(1000, 30000)][int]$TimeoutMs = 15000
)
$ErrorActionPreference = 'Stop'
$clock = [Diagnostics.Stopwatch]::StartNew()
$handles = @{}
$parents = @{}
$parents[$RootPid] = $true

function Capture-Descendants {
    $snapshot = @(Get-CimInstance Win32_Process)
    do {
        $added = $false
        foreach ($entry in $snapshot) {
            $processId = [int]$entry.ProcessId
            if ($processId -eq $PID -or $handles.ContainsKey($processId)) { continue }
            if ($processId -ne $RootPid -and -not $parents.ContainsKey([int]$entry.ParentProcessId)) { continue }
            # Keep ancestry even when the parent exits before the handle lookup.
            if (-not $parents.ContainsKey($processId)) {
                $parents[$processId] = $true
                $added = $true
            }
            $process = $null
            try {
                $process = [Diagnostics.Process]::GetProcessById($processId)
                # Open the handle before any kill. WaitForExit then tracks this process, not a reused PID.
                $null = $process.Handle
                if ($process.HasExited) { $process.Dispose(); continue }
                if ([Math]::Abs(($process.StartTime.ToUniversalTime() - $entry.CreationDate.ToUniversalTime()).TotalMilliseconds) -gt 1) {
                    $process.Dispose()
                    continue
                }
                $handles[$processId] = $process
                $added = $true
            } catch {
                $cause = $_.Exception
                while ($cause.InnerException) { $cause = $cause.InnerException }
                if ($process) { $process.Dispose() }
                # A process can exit between the snapshot and the handle lookup.
                if ($cause -is [ArgumentException] -or $cause -is [InvalidOperationException] -or
                    ($cause -is [ComponentModel.Win32Exception] -and $cause.NativeErrorCode -eq 87)) { continue }
                throw
            }
        }
    } while ($added -and $clock.ElapsedMilliseconds -lt $TimeoutMs)
}

try {
    Capture-Descendants
    # Stop the root first so it cannot start more children.
    if ($handles.ContainsKey($RootPid) -and -not $handles[$RootPid].HasExited) {
        try { $handles[$RootPid].Kill() } catch { if (-not $handles[$RootPid].HasExited) { throw } }
    }
    do {
        $before = $handles.Count
        Capture-Descendants
        foreach ($process in $handles.Values) {
            if (-not $process.HasExited) {
                try { $process.Kill() } catch { if (-not $process.HasExited) { throw } }
            }
        }
        foreach ($process in $handles.Values) {
            $remaining = [Math]::Max(0, $TimeoutMs - $clock.ElapsedMilliseconds)
            if (-not $process.WaitForExit([int]$remaining)) {
                throw 'The native probe descendant did not stop before the deadline.'
            }
        }
        # A child can spawn between the snapshot and the stop.
        Capture-Descendants
        $pending = @($handles.Values | Where-Object { -not $_.HasExited }).Count
        if ($clock.ElapsedMilliseconds -ge $TimeoutMs -and ($pending -gt 0 -or $handles.Count -ne $before)) {
            throw 'The native probe descendants did not stop before the deadline.'
        }
    } while ($pending -gt 0 -or $handles.Count -ne $before)
} finally {
    foreach ($process in $handles.Values) { $process.Dispose() }
}
