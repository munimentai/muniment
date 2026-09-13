param([string]$Directory, [string]$Driver)
$ErrorActionPreference = 'Stop'
$appPath = Join-Path $Directory 'muniment-desktop.exe'
$webviewPath = Join-Path $Directory 'msedgewebview2.exe'
Add-Type -ReferencedAssemblies System.Windows.Forms, System.Drawing -OutputAssembly $appPath -OutputType WindowsApplication -TypeDefinition @'
using System;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

public class PickerFixture {
  [STAThread]
  public static void Main(string[] args) {
    string directory = AppDomain.CurrentDomain.BaseDirectory;
    bool child = args.Length > 0;
    string prefix = child ? "webview" : "app";
    var visible = new Form { Text = prefix + " visible", Size = new Size(240, 120) };
    visible.Show();
    var hidden = new Form { Text = prefix + " hidden", Size = new Size(240, 120) };
    IntPtr hiddenHandle = hidden.Handle;
    var offscreen = new Form { Text = prefix + " offscreen", Size = new Size(240, 120) };
    offscreen.Show();
    offscreen.Location = new Point(-32000, -32000);
    if (!child) Process.Start(new ProcessStartInfo(Path.Combine(directory, "msedgewebview2.exe"), "child") { UseShellExecute = false });
    File.WriteAllText(Path.Combine(directory, prefix + ".ready"), Process.GetCurrentProcess().Id.ToString());
    Application.Run();
    GC.KeepAlive(hiddenHandle);
    GC.KeepAlive(hidden);
  }
}
'@
Copy-Item -LiteralPath $appPath -Destination $webviewPath
$app = Start-Process -FilePath $appPath -PassThru
$null = $app.Handle
try {
  $deadline = [DateTime]::UtcNow.AddSeconds(15)
  while (-not ((Test-Path -LiteralPath (Join-Path $Directory 'app.ready')) -and
               (Test-Path -LiteralPath (Join-Path $Directory 'webview.ready')))) {
    if ($app.HasExited) { throw 'The picker fixture exited before it created its windows.' }
    if ([DateTime]::UtcNow -ge $deadline) { throw 'The picker fixture did not create its windows.' }
    Start-Sleep -Milliseconds 100
  }
  $env:MUNIMENT_FOLDER_PATH = $Directory
  $env:MUNIMENT_FOLDER_WAIT_SECONDS = '1'
  $start = New-Object System.Diagnostics.ProcessStartInfo
  $start.FileName = 'powershell.exe'
  $start.Arguments = "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$Driver`""
  $start.UseShellExecute = $false
  $start.RedirectStandardOutput = $true
  $start.RedirectStandardError = $true
  $drive = [System.Diagnostics.Process]::Start($start)
  try {
    $stdout = $drive.StandardOutput.ReadToEndAsync()
    $stderr = $drive.StandardError.ReadToEndAsync()
    if (-not $drive.WaitForExit(20000)) {
      $drive.Kill()
      throw 'The picker fixture drive exceeded its timeout.'
    }
    Write-Output $stdout.Result
    Write-Output $stderr.Result
    Write-Output "driver exit: $($drive.ExitCode)"
    if ($drive.ExitCode -eq 0) { throw 'The picker drive accepted an app without a dialog.' }
  } finally {
    $drive.Dispose()
  }
} finally {
  try {
    Get-CimInstance Win32_Process -OperationTimeoutSec 5 | Where-Object { $_.ParentProcessId -eq $app.Id } | ForEach-Object {
      $child = Get-Process -Id $_.ProcessId -ErrorAction SilentlyContinue
      if ($child) {
        $null = $child.Handle
        Stop-Process -InputObject $child -Force -ErrorAction SilentlyContinue
        $null = $child.WaitForExit(5000)
        $child.Dispose()
      }
    }
  } finally {
    Stop-Process -Id $app.Id -Force -ErrorAction SilentlyContinue
    $null = $app.WaitForExit(5000)
    $app.Dispose()
  }
}
