$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes

# EnumWindows includes hidden windows that the UI Automation tree can omit.
Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace MunimentFolderPicker {
  public static class Desktop {
    public delegate bool EnumWindowProc(IntPtr handle, IntPtr parameter);
    [DllImport("user32.dll")]
    private static extern bool EnumWindows(EnumWindowProc callback, IntPtr parameter);
    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(IntPtr handle, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetClassName(IntPtr handle, StringBuilder text, int capacity);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetWindowText(IntPtr handle, StringBuilder text, int capacity);
    [DllImport("user32.dll")]
    private static extern bool IsWindowVisible(IntPtr handle);
    [DllImport("user32.dll")]
    private static extern bool GetWindowRect(IntPtr handle, out Rectangle rectangle);
    [DllImport("user32.dll")]
    private static extern IntPtr MonitorFromWindow(IntPtr handle, uint flags);

    [StructLayout(LayoutKind.Sequential)]
    public struct Rectangle { public int Left, Top, Right, Bottom; }
    public class Window {
      public long Handle;
      public uint ProcessId;
      public string Class, Title;
      public bool Visible, Offscreen, RectangleAvailable;
      public Rectangle Rectangle;
    }
    public static Window[] Windows() {
      var windows = new List<Window>();
      EnumWindowProc callback = delegate(IntPtr handle, IntPtr parameter) {
        uint processId;
        GetWindowThreadProcessId(handle, out processId);
        var name = new StringBuilder(512);
        var title = new StringBuilder(32768);
        GetClassName(handle, name, name.Capacity);
        GetWindowText(handle, title, title.Capacity);
        Rectangle rectangle;
        bool available = GetWindowRect(handle, out rectangle);
        windows.Add(new Window {
          Handle = handle.ToInt64(), ProcessId = processId, Class = name.ToString(),
          Title = title.ToString(), Visible = IsWindowVisible(handle),
          Offscreen = MonitorFromWindow(handle, 0) == IntPtr.Zero,
          Rectangle = rectangle, RectangleAvailable = available
        });
        return true;
      };
      if (!EnumWindows(callback, IntPtr.Zero)) throw new InvalidOperationException("The desktop window query failed.");
      return windows.ToArray();
    }
  }
}
'@

$homePath = $env:MUNIMENT_FOLDER_PATH
$deadline = [DateTime]::UtcNow.AddSeconds([int]$env:MUNIMENT_FOLDER_WAIT_SECONDS)
$descendants = [System.Windows.Automation.TreeScope]::Descendants

function Write-PickerWindows {
  $appIds = @(Get-Process -Name 'muniment-desktop' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  $processes = @()
  try {
    $processes = @(Get-CimInstance Win32_Process -OperationTimeoutSec 5 -ErrorAction Stop)
  } catch {
    Write-Output "process tree error: $($_.Exception.Message)"
  }
  $family = @($appIds)
  do {
    $added = @($processes | Where-Object { $_.ParentProcessId -in $family -and $_.ProcessId -notin $family })
    $family += @($added | ForEach-Object { $_.ProcessId })
  } while ($added.Count -gt 0)
  $webviews = @($processes | Where-Object { $_.ProcessId -in $family -and $_.Name -ieq 'msedgewebview2.exe' })
  $processIds = @($appIds) + @($webviews | ForEach-Object { $_.ProcessId })
  Write-Output "app processes: $($appIds -join ', '). WebView2 processes: $(($webviews | ForEach-Object { $_.ProcessId }) -join ', ')"
  $windows = @([MunimentFolderPicker.Desktop]::Windows())
  foreach ($processId in $processIds) {
    $process = $processes | Where-Object { $_.ProcessId -eq $processId } | Select-Object -First 1
    $owned = @($windows | Where-Object { $_.ProcessId -eq $processId })
    Write-Output "process: $processId. name: $($process.Name). parent: $($process.ParentProcessId). top-level windows: $($owned.Count)"
    foreach ($window in $owned) {
      Write-Output ('window: ' + ($window | ConvertTo-Json -Compress -Depth 3))
    }
  }
}

function Wait-PickerStep([string]$step) {
  if ([DateTime]::UtcNow -ge $deadline) { throw "The shell folder dialog timed out during $step." }
  Start-Sleep -Milliseconds 100
}

function Find-FolderDialog {
  $processIds = @(Get-Process -Name 'muniment-desktop' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
  $matches = @([MunimentFolderPicker.Desktop]::Windows() | Where-Object {
    $_.ProcessId -in $processIds -and $_.Class -eq '#32770'
  })
  if ($matches.Count -gt 1) { throw 'Muniment has more than one shell dialog.' }
  if ($matches.Count -eq 1) {
    return [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$matches[0].Handle)
  }
}

try {
  $dialog = Find-FolderDialog
  while ($null -eq $dialog) {
    Wait-PickerStep 'the window search'
    $dialog = Find-FolderDialog
  }
  Write-Output "window: $($dialog.Current.Name). process: $($dialog.Current.ProcessId)"
  $handle = $dialog.Current.NativeWindowHandle
  $editCondition = [System.Windows.Automation.AndCondition]::new(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '1152'),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Edit)
  )
  $confirmCondition = [System.Windows.Automation.AndCondition]::new(
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '1'),
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)
  )
  $edit = $dialog.FindFirst($descendants, $editCondition)
  while ($null -eq $edit -or -not $edit.Current.IsEnabled -or $edit.Current.IsOffscreen) {
    Wait-PickerStep 'the Folder field search'
    $edit = $dialog.FindFirst($descendants, $editCondition)
  }
  $value = $edit.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
  $value.SetValue($homePath)
  if ($value.Current.Value -cne $homePath) { throw 'The Folder field did not keep the Home path.' }
  $confirm = $dialog.FindFirst($descendants, $confirmCondition)
  while ($null -eq $confirm -or -not $confirm.Current.IsEnabled -or $confirm.Current.IsOffscreen) {
    Wait-PickerStep 'the Select Folder button search'
    $confirm = $dialog.FindFirst($descendants, $confirmCondition)
  }
  $confirm.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  while ($null -ne ([MunimentFolderPicker.Desktop]::Windows() | Where-Object {
    $_.Handle -eq $handle -and $_.Visible
  })) {
    Wait-PickerStep 'the window close'
  }
} catch {
  $failure = $_
  try {
    Write-PickerWindows
  } catch {
    Write-Output "window report error: $($_.Exception.Message)"
  }
  throw $failure
}
