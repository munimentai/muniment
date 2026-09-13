# The disposable guest checks HTTPS time without changing the device-proof contract.
function Get-TrustedClockSample {
  $source = 'https://api.muniment.ai/'
  $request = [Net.HttpWebRequest]::Create("${source}?clock_probe=$([guid]::NewGuid().ToString('N'))")
  $request.Method = 'HEAD'
  $request.AllowAutoRedirect = $false
  $request.Timeout = 10000
  $request.ReadWriteTimeout = 10000
  $request.Headers['Cache-Control'] = 'no-cache, no-store'
  $request.CachePolicy = New-Object Net.Cache.RequestCachePolicy ([Net.Cache.RequestCacheLevel]::NoCacheNoStore)
  $response = $null
  $started = [DateTime]::UtcNow
  $watch = [Diagnostics.Stopwatch]::StartNew()
  try {
    try {
      $response = $request.GetResponse()
    } catch [Net.WebException] {
      # An HTTP rejection still carries the trusted HTTPS hop's Date header.
      $response = $_.Exception.Response
      if ($null -eq $response) { throw 'The trusted clock source did not answer over HTTPS.' }
    }
    $watch.Stop()
    $finished = [DateTime]::UtcNow
    if ($watch.Elapsed.TotalSeconds -gt 5) { throw 'The trusted clock sample took more than five seconds.' }
    if ([Math]::Abs(($finished - $started).TotalSeconds - $watch.Elapsed.TotalSeconds) -gt 1) {
      throw 'The clone clock changed during the trusted clock sample.'
    }
    $age = $response.Headers['Age']
    if ($age -and $age -ne '0') { throw 'The trusted clock source returned a cached sample.' }
    $trusted = [DateTimeOffset]::MinValue
    $styles = [Globalization.DateTimeStyles]::AssumeUniversal -bor [Globalization.DateTimeStyles]::AdjustToUniversal
    if (-not [DateTimeOffset]::TryParseExact($response.Headers['Date'], 'r', [Globalization.CultureInfo]::InvariantCulture, $styles, [ref]$trusted)) {
      throw 'The trusted clock source returned no valid Date header.'
    }
    # HTTP Date has one-second precision. The round trip bounds the sample uncertainty.
    $uncertainty = 1 + $watch.Elapsed.TotalSeconds / 2
    $reference = $trusted.UtcDateTime.AddSeconds($watch.Elapsed.TotalSeconds / 2)
    return [pscustomobject]@{
      Source = $source
      LocalUtc = $finished
      TrustedUtc = $reference
      OffsetSeconds = ($reference - $finished).TotalSeconds
      UncertaintySeconds = $uncertainty
    }
  } finally {
    if ($null -ne $response) { $response.Close() }
  }
}

function Write-ClockSample($Sample, [string]$Phase, [string]$Log) {
  $line = "clock phase=$Phase source=$($Sample.Source) local_utc=$($Sample.LocalUtc.ToString('o')) trusted_utc=$($Sample.TrustedUtc.ToString('o')) offset_seconds=$($Sample.OffsetSeconds) uncertainty_seconds=$($Sample.UncertaintySeconds) timezone=$([TimeZoneInfo]::Local.Id)"
  Add-Content -LiteralPath $Log -Value $line
  Write-Host $line
}

function Sync-SignInClock([string]$Log) {
  try {
    $sample = Get-TrustedClockSample
    Write-ClockSample $sample 'before' $Log
    if ([Math]::Abs($sample.OffsetSeconds) + $sample.UncertaintySeconds -gt 10) {
      try {
        Set-Date -Date $sample.TrustedUtc.ToLocalTime() -ErrorAction Stop | Out-Null
      } catch {
        throw 'The runner could not set the clone clock. Grant the guest account the system-time privilege.'
      }
    }
    # A fresh sample proves the correction instead of trusting Set-Date's return value.
    $sample = Get-TrustedClockSample
    Write-ClockSample $sample 'verified' $Log
    if ([Math]::Abs($sample.OffsetSeconds) + $sample.UncertaintySeconds -gt 10) {
      throw 'The clone clock exceeds the ten-second sign-in limit.'
    }
  } catch {
    Add-Content -LiteralPath $Log -Value "clock failure=$($_.Exception.Message)"
    throw
  }
}
